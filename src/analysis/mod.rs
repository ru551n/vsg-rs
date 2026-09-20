//! Static analysis: what follows from the resolved program, as opposed to how it is laid out.
//!
//! The style layer ([`crate::rules`]) decides how source should look, one file at a time. This
//! layer asks whether the design says what its author meant: a signal nothing drives, two
//! statements driving one signal, a state a machine cannot leave. Some rules need only the syntax
//! tree; the rest need names resolved across files, which [`lint`] gets from the VHDL front end.
//!
//! Each rule states the evidence it works from, and reports nothing when that evidence is
//! missing. See `docs/lint.md`.

pub mod calls;
pub mod choices;
pub mod clockdomain;
pub mod combinational;
pub mod design;
pub mod elaborate;
pub mod evaluated;
pub mod fsm;
pub mod lint;
pub mod suspend;
pub mod testbench;
pub mod width;

use std::path::Path;

use crate::{Config, Parsed};

/// How sure a rule is that what it reports is wrong.
///
/// This is the difference between "your program cannot do what it says" and "you may not have
/// meant this". A default run reports only the first, so that a finding is something to correct
/// rather than something to triage -- the whole value of the report is that it does not need
/// reading twice.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Certainty {
    /// The program is wrong: the analyser can show that what is written cannot be carried out.
    /// A definite finding does not depend on what anyone intended. **On by default.**
    Definite,
    /// The fact is exact and the conclusion is a judgement. An unused declaration really is
    /// unused; whether that is a mistake is not something the source says. **Off by default.**
    Advisory,
    /// Inferred rather than derived: the rule decides what the design is trying to be before it
    /// decides whether it succeeds. **Off by default.**
    Experimental,
    /// A house's convention, which the language has no opinion about. **Off by default.**
    Policy,
}

impl Certainty {
    /// The configuration group that switches the whole class on or off.
    #[must_use]
    pub fn group(self) -> &'static str {
        match self {
            Certainty::Definite => "definite",
            Certainty::Advisory => "advisory",
            Certainty::Experimental => "experimental",
            Certainty::Policy => "policy",
        }
    }

    /// Whether a run that was told nothing reports this class.
    #[must_use]
    pub fn on_by_default(self) -> bool {
        self == Certainty::Definite
    }

    /// What the class is called in a report or on the command line.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Certainty::Definite => "definite error",
            Certainty::Advisory => "advisory",
            Certainty::Experimental => "experimental",
            Certainty::Policy => "policy",
        }
    }
}

/// One rule of the lint layer: what it is called, what it reports, and how sure it is.
#[derive(Copy, Clone, Debug)]
pub struct Rule {
    pub id: &'static str,
    pub description: &'static str,
    pub certainty: Certainty,
}

/// Every rule of the lint layer, from the modules that implement them.
///
/// The one list. What a rule reports, whether a default run runs it, what the documentation
/// says about it and how a report classifies it all read this, so none of them can drift from
/// another.
pub fn rules() -> impl Iterator<Item = Rule> {
    lint::rules()
        .chain(design::RULES.iter().copied())
        .chain(elaborate::RULES.iter().copied())
        .chain(fsm::RULES.iter().copied())
        .chain(combinational::RULES.iter().copied())
        .chain(clockdomain::RULES.iter().copied())
        .chain(width::RULES.iter().copied())
        .chain(choices::RULES.iter().copied())
        .chain(calls::RULES.iter().copied())
        .chain(suspend::RULES.iter().copied())
        .chain(evaluated::RULES.iter().copied())
}

/// How sure the rule with this id is, or `None` if it is not a rule of the lint layer.
#[must_use]
pub fn certainty_of(id: &str) -> Option<Certainty> {
    rules()
        .find(|rule| rule.id == id)
        .map(|rule| rule.certainty)
}

/// How a project wants registers named (`lint_602`, `lint_603`), from its configuration. Both
/// rules are off unless configured, and a rule that is off contributes nothing.
///
/// # Errors
/// If a configured prefix or suffix is not a valid pattern.
pub fn naming_rules(cfg: &Config) -> Result<design::Naming, String> {
    let affixes = |rule: &str, key: &str, fallback: &[&str]| -> Result<Vec<_>, String> {
        let Some(settings) = cfg.rule_by_id(rule).filter(|s| s.enabled) else {
            return Ok(Vec::new());
        };
        let configured = settings.option_list(key);
        let listed: Vec<String> = if configured.is_empty() {
            fallback.iter().map(|s| (*s).to_owned()).collect()
        } else {
            configured.iter().map(|s| s.to_ascii_lowercase()).collect()
        };
        listed
            .iter()
            .map(|affix| design::Affix::new(affix).map_err(|e| format!("{rule}: {e}")))
            .collect()
    };
    Ok(design::Naming {
        suffixes: affixes("lint_602", "suffixes", &["_q", "_r", "_reg"])?,
        prefixes: affixes("lint_603", "prefixes", &["r_"])?,
    })
}

/// Every rule of the lint layer that needs nothing but this file, with the configuration applied.
///
/// This is what one buffer can be told on its own. The rules that resolve names across files
/// ([`lint::analyse`]) and the one that needs every entity's ports ([`elaborate::undriven`]) are
/// not here, because neither can be answered from a single file.
#[must_use]
pub fn findings_for(parsed: &Parsed, path: &Path, cfg: &Config) -> Vec<lint::Finding> {
    let naming = naming_rules(cfg).unwrap_or(design::Naming {
        prefixes: Vec::new(),
        suffixes: Vec::new(),
    });
    per_file(parsed, path, &naming, &cfg.synchronizers)
        .into_iter()
        .filter(|f| {
            cfg.rule_by_id(f.rule)
                .is_none_or(|settings| settings.enabled)
        })
        .collect()
}

/// Every single-file rule, with nothing filtered out.
///
/// This is the one list of those rules. The command line adds the project-wide rules to it and
/// applies its own per-file configuration; [`findings_for`] applies one configuration and stops
/// there. Both go through here, so a rule added to this function reaches both -- while the two
/// kept their own lists, a rule added to one was simply missing from the other.
#[must_use]
pub fn per_file(
    parsed: &Parsed,
    path: &Path,
    naming: &design::Naming,
    synchronizers: &[String],
) -> Vec<lint::Finding> {
    design::check(parsed, path, naming)
        .into_iter()
        .chain(fsm::check(parsed, path))
        .chain(combinational::check(parsed, path))
        .chain(clockdomain::check(parsed, path, synchronizers))
        .chain(width::check(parsed, path))
        .chain(choices::check(parsed, path))
        .chain(suspend::check(parsed, path))
        .chain(evaluated::check(parsed, path))
        .collect()
}

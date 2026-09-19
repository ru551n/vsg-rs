//! Static analysis: what follows from the resolved program, as opposed to how it is laid out.
//!
//! The style layer ([`crate::rules`]) decides how source should look, one file at a time. This
//! layer asks whether the design says what its author meant: a signal nothing drives, two
//! statements driving one signal, a state a machine cannot leave. Some rules need only the syntax
//! tree; the rest need names resolved across files, which [`lint`] gets from the VHDL front end.
//!
//! Each rule states the evidence it works from, and reports nothing when that evidence is
//! missing. See `docs/lint.md`.

pub mod clockdomain;
pub mod combinational;
pub mod design;
pub mod elaborate;
pub mod fsm;
pub mod lint;
pub mod testbench;
pub mod width;

use std::path::Path;

use crate::{Config, Parsed};

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
    let synchronizers = &cfg.synchronizers;
    design::check(parsed, path, &naming)
        .into_iter()
        .chain(fsm::check(parsed, path))
        .chain(combinational::check(parsed, path))
        .chain(clockdomain::check(parsed, path, synchronizers))
        .chain(width::check(parsed, path))
        .filter(|f| {
            cfg.rule_by_id(f.rule)
                .is_none_or(|settings| settings.enabled)
        })
        .collect()
}

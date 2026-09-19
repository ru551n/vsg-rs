//! Rule engine: rules read one parsed snapshot and report violations with optional fixes.
//!
//! Rules never modify the source and never depend on each other. Results are sorted by
//! position and rule id, so the order in which rules run is irrelevant.

mod case;
mod catalog;
mod length;
mod naming;
mod project;
mod select;
mod structure;
mod transform;

use std::cell::OnceCell;
use std::collections::HashMap;
use std::fmt;

use vhdl_syntax::syntax::{NodeKind, SyntaxNode, SyntaxToken};

pub use project::Project;

use crate::Parsed;
use crate::config::{Config, RuleSettings, Severity};

/// Which part of vsg-rs is responsible for a VSG rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// Layout policy, enforced by `vsg-rs fmt`.
    Formatter,
    /// Structural rule whose fix is a syntax-local edit.
    Structure,
    /// Naming, case and comment policy.
    Lint,
    /// Needs name resolution across declarations.
    Semantic,
    /// Reported by the command line (missing input files).
    Cli,
}

impl fmt::Display for Owner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Owner::Formatter => "formatter",
            Owner::Structure => "structure",
            Owner::Lint => "lint",
            Owner::Semantic => "semantic",
            Owner::Cli => "command line",
        })
    }
}

/// Static description of an implemented rule.
#[derive(Debug)]
pub struct RuleInfo {
    pub id: &'static str,
    /// VSG rule groups the rule belongs to (for `rule.group` configuration).
    pub groups: &'static [&'static str],
    pub severity: Severity,
    pub enabled_by_default: bool,
    pub description: &'static str,
}

impl RuleInfo {
    /// A rule the formatter applies: its settings configure the layout, and it has no check.
    pub(crate) const fn formatter(
        id: &'static str,
        groups: &'static [&'static str],
        enabled_by_default: bool,
        description: &'static str,
    ) -> RuleInfo {
        RuleInfo {
            id,
            groups,
            severity: Severity::Error,
            enabled_by_default,
            description,
        }
    }
}

pub(crate) type Check = fn(&Context<'_>, &RuleSettings, &mut Vec<Violation>);

pub(crate) struct Rule {
    pub(crate) info: RuleInfo,
    pub(crate) check: Check,
}

/// How safe it is to apply a fix without review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixSafety {
    /// Syntax-local and meaning-preserving; applied by `vsg-rs fix`.
    Safe,
    /// May change behaviour; shown as a suggestion only.
    Unsafe,
}

/// Replace `start..end` of the original source with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
    /// Orders insertions at the same offset (lower first).
    pub rank: u8,
}

#[derive(Debug, Clone)]
pub struct Fix {
    pub safety: FixSafety,
    pub edits: Vec<Edit>,
}

#[derive(Debug, Clone)]
pub struct Violation {
    pub rule: &'static str,
    pub severity: Severity,
    /// Byte range in the source.
    pub start: usize,
    pub end: usize,
    pub message: String,
    pub fix: Option<Fix>,
}

/// Shared, lazily built information about one snapshot.
pub(crate) struct Context<'a> {
    pub(crate) parsed: &'a Parsed,
    pub(crate) config: &'a Config,
    by_kind: HashMap<NodeKind, Vec<SyntaxNode>>,
    /// The formatted snapshot, parsed; computed on first use.
    formatted: OnceCell<Result<Parsed, crate::FormatError>>,
    /// The snapshot is known to be formatted already.
    canonical: bool,
    /// Identifier use sites (not declarations or selected suffixes): offset, token, lower case.
    use_sites: OnceCell<Vec<(usize, SyntaxToken, Box<str>)>>,
    /// Declarations of the other files checked together with this one.
    pub(crate) project: Option<&'a Project>,
    /// Lower-case names with the kinds of declaration that declare them in this file.
    declared_kinds: OnceCell<HashMap<String, Vec<&'static str>>>,
}

impl<'a> Context<'a> {
    fn new(
        parsed: &'a Parsed,
        config: &'a Config,
        canonical: bool,
        project: Option<&'a Project>,
    ) -> Self {
        let mut by_kind: HashMap<NodeKind, Vec<SyntaxNode>> = HashMap::new();
        let mut stack = vec![parsed.root().clone()];
        while let Some(n) = stack.pop() {
            stack.extend(n.children());
            by_kind.entry(n.kind()).or_default().push(n);
        }
        // Source order, independent of traversal order.
        for nodes in by_kind.values_mut() {
            nodes.sort_by_key(SyntaxNode::offset);
        }
        Context {
            parsed,
            config,
            by_kind,
            formatted: OnceCell::new(),
            canonical,
            use_sites: OnceCell::new(),
            project,
            declared_kinds: OnceCell::new(),
        }
    }

    pub(crate) fn nodes(&self, kind: NodeKind) -> &[SyntaxNode] {
        self.by_kind.get(&kind).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn use_sites(&self) -> &[(usize, SyntaxToken, Box<str>)] {
        self.use_sites.get_or_init(|| {
            self.parsed
                .tokens()
                .iter()
                .filter(|t| {
                    t.kind() == vhdl_syntax::tokens::TokenKind::Identifier
                        && t.text().as_bytes().first() != Some(&b'\\')
                        && !select::is_declaration_position(t)
                        && !select::is_qualified_position(t)
                })
                .map(|t| {
                    let lower = String::from_utf8_lossy(t.text().as_bytes()).to_ascii_lowercase();
                    (t.text_offset(), t.clone(), lower.into_boxed_str())
                })
                .collect()
        })
    }

    pub(crate) fn declared_kinds(&self) -> &HashMap<String, Vec<&'static str>> {
        self.declared_kinds
            .get_or_init(|| case::declared_kinds(self))
    }

    /// The canonical formatting of this snapshot, parsed once on first use.
    pub(crate) fn formatted(&self) -> Option<&Parsed> {
        if self.canonical {
            return Some(self.parsed);
        }
        self.format_result().as_ref().ok()
    }

    fn format_result(&self) -> &Result<Parsed, crate::FormatError> {
        self.formatted
            .get_or_init(|| crate::format_parsed(self.parsed, &self.config.format).map(Parsed::new))
    }
}

pub(crate) fn violation(
    settings: &RuleSettings,
    rule: &'static str,
    at: &SyntaxToken,
    message: impl Into<String>,
) -> Violation {
    let range = at.text_range();
    Violation {
        rule,
        severity: settings.severity,
        start: range.start,
        end: range.end,
        message: message.into(),
        fix: None,
    }
}

fn rules() -> &'static [Rule] {
    static RULES: std::sync::OnceLock<Vec<Rule>> = std::sync::OnceLock::new();
    RULES.get_or_init(|| {
        let mut all = Vec::new();
        all.extend(case::rules());
        all.extend(length::rules());
        all.extend(naming::rules());
        all.extend(structure::rules());
        all.extend(transform::rules());
        all.sort_by_key(|r| r.info.id);
        all
    })
}

/// Description of an implemented rule.
pub fn info(id: &str) -> Option<&'static RuleInfo> {
    rules().iter().map(|r| &r.info).find(|i| i.id == id)
}

/// Every VSG rule id with the part of vsg-rs that owns it (the formatter, unless
/// `catalog::OWNERS` says otherwise).
pub fn vsg_catalog() -> impl Iterator<Item = (&'static str, Owner)> {
    crate::vsg_defaults::rule_ids()
        .filter(|id| *id != "global")
        .map(|id| (id, owner(id)))
}

/// Which part of vsg-rs owns a rule.
#[must_use]
pub fn owner_of(id: &str) -> Owner {
    owner(id)
}

fn owner(id: &str) -> Owner {
    catalog::OWNERS
        .binary_search_by_key(&id, |(r, _)| r)
        .map_or(Owner::Formatter, |i| catalog::OWNERS[i].1)
}

pub fn is_known_rule(id: &str) -> bool {
    // `lint_*` are vsg-rs's own rules (the lint layer); the rest are VSG's.
    id.starts_with("lint_") || !crate::vsg_defaults::defaults()["rule"][id].is_null()
}

/// Run every enabled rule on a snapshot. Violations are sorted by position, then rule id.
pub fn check(parsed: &Parsed, config: &Config) -> Vec<Violation> {
    check_with(parsed, config, None)
}

/// [`check`], with the declarations of other files checked in the same run.
pub fn check_with(parsed: &Parsed, config: &Config, project: Option<&Project>) -> Vec<Violation> {
    run(&Context::new(parsed, config, false, project))
}

/// [`check`] plus the formatted source, formatting the snapshot at most once.
pub fn check_and_format_with(
    parsed: &Parsed,
    config: &Config,
    project: Option<&Project>,
) -> (Vec<Violation>, Result<Vec<u8>, crate::FormatError>) {
    let cx = Context::new(parsed, config, false, project);
    let violations = run(&cx);
    let _ = cx.format_result();
    let formatted = cx
        .formatted
        .into_inner()
        .expect("initialized above")
        .map(|p| p.source().to_vec());
    (violations, formatted)
}

/// [`check`] without the formatted snapshot: for collecting fixes (formatter-owned violations
/// have none) and for a snapshot that is the formatter's own output.
pub(crate) fn check_unformatted(
    parsed: &Parsed,
    config: &Config,
    project: Option<&Project>,
) -> Vec<Violation> {
    run(&Context::new(parsed, config, true, project))
}

fn run(cx: &Context<'_>) -> Vec<Violation> {
    let mut out = Vec::new();
    for rule in rules() {
        let settings = cx.config.rule(&rule.info);
        if !settings.enabled {
            continue;
        }
        let before = out.len();
        (rule.check)(cx, &settings, &mut out);
        if !settings.fixable {
            for v in &mut out[before..] {
                if settings.fixable_configured {
                    v.fix = None;
                } else if let Some(fix) = &mut v.fix {
                    // VSG does not fix this rule by default: a suggestion for `--unsafe-fixes`.
                    fix.safety = FixSafety::Unsafe;
                }
            }
        }
    }
    let suppressions = suppressions(cx.parsed);
    if !suppressions.is_empty() {
        out.retain(|v| !suppressed(&suppressions, v));
    }
    out.sort_by(|a, b| (a.start, a.rule, a.end).cmp(&(b.start, b.rule, b.end)));
    out
}

/// A VSG `-- vsg_off [rule ...]` (`on == false`) or `-- vsg_on [rule ...]` comment. An empty
/// rule list applies to all rules.
struct Suppression {
    offset: usize,
    on: bool,
    rules: Vec<String>,
}

fn suppressions(parsed: &Parsed) -> Vec<Suppression> {
    let mut out = Vec::new();
    for t in parsed.tokens() {
        for piece in t.leading_trivia() {
            let vhdl_syntax::tokens::TriviaPiece::LineComment(c) = piece else {
                continue;
            };
            let text = String::from_utf8_lossy(c.as_bytes()).to_ascii_lowercase();
            let mut words = text.trim_start_matches('-').split_whitespace();
            let on = match words.next() {
                Some("vsg_off") => false,
                Some("vsg_on") => true,
                _ => continue,
            };
            out.push(Suppression {
                offset: t.text_offset(),
                on,
                rules: words.map(str::to_owned).collect(),
            });
        }
    }
    out
}

fn suppressed(suppressions: &[Suppression], v: &Violation) -> bool {
    let mut off = false;
    for s in suppressions.iter().take_while(|s| s.offset <= v.start) {
        if s.rules.is_empty() || s.rules.iter().any(|r| r == v.rule) {
            off = !s.on;
        }
    }
    off
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vsg_off_comments_suppress_rules() {
        let src = "entity a is\nend;\n-- vsg_off entity_019\nentity b is\nend;\n-- vsg_on\n-- vsg_off\nentity c is\nend;\n-- vsg_on\nentity d is\nend;\n";
        let starts = ["entity a", "entity b", "entity c", "entity d"].map(|e| src.find(e).unwrap());
        let found: Vec<(usize, &str)> =
            check(&Parsed::new(src.as_bytes().to_vec()), &Config::default())
                .into_iter()
                .map(|v| (starts.iter().rposition(|s| *s <= v.start).unwrap(), v.rule))
                .collect();
        assert_eq!(
            found,
            [
                (0, "entity_015"),
                (0, "entity_019"),
                (1, "entity_015"),
                (3, "entity_015"),
                (3, "entity_019"),
            ]
        );
    }

    #[test]
    fn catalog_is_sorted_and_contains_implemented_rules() {
        assert!(catalog::OWNERS.windows(2).all(|w| w[0].0 < w[1].0));
        for info in rules().iter().map(|r| &r.info) {
            assert!(is_known_rule(info.id), "{} is not a VSG rule", info.id);
        }
    }
}

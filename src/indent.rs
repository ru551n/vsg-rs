//! VSG's `indent.tokens` configuration mapped onto the formatter's structural indentation.
//!
//! VSG describes indentation per token: `token` is the indent of the token itself and `after`
//! the indent of what follows, either absolute (`0`, `1`) or relative to the current indent
//! (`"current"`, `"+1"`, `"-1"`). For each construct this module replays those settings from
//! the construct's own level and records the resulting relative levels of its parts (body,
//! `begin`, statements, branches, `end`). Settings that do not reduce to such levels are
//! reported as unsupported.

use std::collections::HashMap;

use serde_json::Value;
use vhdl_syntax::syntax::NodeKind as N;

/// A part of a construct whose indentation the formatter decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    /// Declarations, clauses or statements directly inside the construct.
    Body,
    /// `begin`.
    Begin,
    /// Statements after `begin`.
    Statements,
    /// `elsif` / `else` branches.
    Branch,
    /// `end` or the closing parenthesis.
    End,
}

impl Part {
    fn default_level(self) -> usize {
        match self {
            Part::Body | Part::Statements => 1,
            Part::Begin | Part::Branch | Part::End => 0,
        }
    }
}

/// Indentation levels (relative to the construct's first line) that differ from the defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndentPolicy {
    levels: HashMap<(N, Part), usize>,
    /// Levels of `use` clauses that follow a `library` clause (default 1).
    pub use_after_library: Option<usize>,
}

impl IndentPolicy {
    pub fn level(&self, construct: N, part: Part) -> usize {
        self.levels
            .get(&(construct, part))
            .copied()
            .unwrap_or_else(|| part.default_level())
    }
}

#[derive(Clone, Copy)]
enum Setting {
    Relative(i64),
    Absolute(i64),
}

fn parse(v: &Value) -> Option<Setting> {
    match v {
        Value::Number(n) => n.as_i64().map(Setting::Absolute),
        Value::String(s) if s == "current" => Some(Setting::Relative(0)),
        Value::String(s) => s
            .strip_prefix('+')
            .unwrap_or(s)
            .parse()
            .ok()
            .map(Setting::Relative),
        _ => None,
    }
}

type Key = (&'static str, &'static str);

struct Resolver<'a> {
    user: &'a Value,
    defaults: &'a Value,
    policy: IndentPolicy,
    warnings: Vec<String>,
    /// The (construct, token) entries this module interprets.
    used: Vec<Key>,
}

impl Resolver<'_> {
    fn get(&mut self, (construct, token): Key, field: &str) -> Setting {
        if !self.used.contains(&(construct, token)) {
            self.used.push((construct, token));
        }
        parse(&self.user[construct][token][field])
            .or_else(|| parse(&self.defaults[construct][token][field]))
            .unwrap_or(Setting::Relative(0))
    }

    /// The indent after applying `s` to `current`. Absolute settings are only meaningful for
    /// design units, which start at level 0.
    fn apply(&mut self, current: i64, s: Setting, design_unit: bool, name: &str) -> Option<i64> {
        match s {
            Setting::Relative(d) => Some(current + d),
            Setting::Absolute(a) if design_unit => Some(a),
            Setting::Absolute(_) => {
                self.warnings.push(format!(
                    "indent.tokens.{name}: absolute indentation is only supported for design units"
                ));
                None
            }
        }
    }

    fn set(&mut self, construct: N, part: Part, level: i64, name: &str) {
        match usize::try_from(level) {
            Ok(level) if level == part.default_level() => {
                self.policy.levels.remove(&(construct, part));
            }
            Ok(level) => {
                self.policy.levels.insert((construct, part), level);
            }
            Err(_) => self.warnings.push(format!(
                "indent.tokens.{name}: a part would be indented left of its construct; not supported"
            )),
        }
    }

    /// A construct with a body, an optional `begin` and an `end`.
    fn block(&mut self, kind: N, name: &'static str, open: Key, begin: Option<Key>, unit: bool) {
        let open_after = self.get(open, "after");
        let Some(body) = self.apply(0, open_after, unit, name) else {
            return;
        };
        let mut current = body;
        let mut levels = vec![(Part::Body, body)];
        if let Some(key) = begin {
            let token = self.get(key, "token");
            let after = self.get(key, "after");
            let (Some(b), Some(s)) = (
                self.apply(current, token, unit, name),
                self.apply(current, after, unit, name),
            ) else {
                return;
            };
            levels.push((Part::Begin, b));
            levels.push((Part::Statements, s));
            current = s;
        }
        let end_token = self.get((name, "end_keyword"), "token");
        let Some(end) = self.apply(current, end_token, unit, name) else {
            return;
        };
        levels.push((Part::End, end));
        for (part, level) in levels {
            self.set(kind, part, level, name);
        }
    }

    /// A construct with branches (`elsif`, `when`) between the construct and its bodies.
    /// `when_branches`: every body is inside a branch (`case`).
    fn branches(
        &mut self,
        kind: N,
        branch_kinds: &[N],
        name: &'static str,
        open: Key,
        branch: Key,
        when_branches: bool,
    ) {
        let open_after = self.get(open, "after");
        let branch_token = self.get(branch, "token");
        let end_token = self.get((name, "end_keyword"), "token");
        let Some(body) = self.apply(0, open_after, false, name) else {
            return;
        };
        let (Some(b), Some(end)) = (
            self.apply(body, branch_token, false, name),
            self.apply(body, end_token, false, name),
        ) else {
            return;
        };
        if when_branches {
            self.set(kind, Part::Body, b, name);
        } else {
            self.set(kind, Part::Body, body, name);
            self.set(kind, Part::Branch, b, name);
        }
        for &branch_kind in branch_kinds {
            self.set(branch_kind, Part::Body, body - b, name);
            self.set(branch_kind, Part::Statements, body - b, name);
        }
        self.set(kind, Part::End, end, name);
    }

    /// `generic (…)`, `port map (…)`: elements and the closing parenthesis.
    fn list(&mut self, kind: N, name: &'static str, keyword: &'static str) {
        let after = self.get((name, keyword), "after");
        let close = self.get((name, "close_parenthesis"), "token");
        let Some(body) = self.apply(0, after, false, name) else {
            return;
        };
        let Some(end) = self.apply(body, close, false, name) else {
            return;
        };
        self.set(kind, Part::Body, body, name);
        self.set(kind, Part::End, end, name);
    }

    /// Everything the user changed that this module does not interpret.
    fn unsupported(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (construct, entries) in self.user.as_object().into_iter().flatten() {
            for (token, fields) in entries.as_object().into_iter().flatten() {
                let understood = self.used.iter().any(|(c, t)| c == construct && t == token);
                if !understood && *fields != self.defaults[construct][token] {
                    out.push(format!("{construct}.{token}"));
                }
            }
        }
        out
    }
}

/// Resolve the effective `indent.tokens` (the user's settings over VSG's defaults).
pub fn resolve(user: Option<&Value>) -> (IndentPolicy, Vec<String>) {
    use N::*;
    let empty = Value::Null;
    let mut r = Resolver {
        user: user.unwrap_or(&empty),
        defaults: &crate::vsg_defaults::defaults()["indent"]["tokens"],
        policy: IndentPolicy::default(),
        warnings: Vec::new(),
        used: Vec::new(),
    };
    let arch = "architecture_body";
    r.block(
        ArchitectureBody,
        arch,
        (arch, "architecture_keyword"),
        Some((arch, "begin_keyword")),
        true,
    );
    let entity = "entity_declaration";
    r.block(
        EntityDeclaration,
        entity,
        (entity, "entity_keyword"),
        Some((entity, "begin_keyword")),
        true,
    );
    let simple: [(N, &'static str, &'static str, bool); 9] = [
        (
            ContextDeclaration,
            "context_declaration",
            "context_keyword",
            true,
        ),
        (
            PackageDeclaration,
            "package_declaration",
            "package_keyword",
            true,
        ),
        (PackageBody, "package_body", "package_keyword", true),
        (
            ComponentDeclaration,
            "component_declaration",
            "component_keyword",
            false,
        ),
        (LoopStatement, "loop_statement", "loop_keyword", false),
        (
            RecordTypeDefinition,
            "record_type_definition",
            "record_keyword",
            false,
        ),
        (
            ProtectedTypeDeclaration,
            "protected_type_declaration",
            "protected_keyword",
            false,
        ),
        (
            ProtectedTypeBody,
            "protected_type_body",
            "body_keyword",
            false,
        ),
        (
            ComponentInstantiationStatement,
            "component_instantiation_statement",
            "instantiation_label",
            false,
        ),
    ];
    for (kind, name, keyword, unit) in simple {
        if kind == ComponentInstantiationStatement {
            let after = r.get((name, keyword), "after");
            if let Some(level) = r.apply(0, after, false, name) {
                r.set(kind, Part::Body, level, name);
            }
        } else {
            r.block(kind, name, (name, keyword), None, unit);
        }
    }
    // Constructs with a `begin` between the declarations and the statements. The opening
    // keyword and the body do not always belong to the construct's own settings.
    let generate_begin = ("generate_statement_body", "begin_keyword");
    for (kind, name, open, begin) in [
        (
            ProcessStatement,
            "process_statement",
            ("process_statement", "process_keyword"),
            ("process_statement", "begin_keyword"),
        ),
        (
            BlockStatement,
            "block_statement",
            ("block_statement", "block_label"),
            ("block_statement", "begin_keyword"),
        ),
        (
            SubprogramBody,
            "subprogram_body",
            ("function_specification", "function_keyword"),
            ("subprogram_body", "begin_keyword"),
        ),
        (
            ForGenerateStatement,
            "for_generate_statement",
            ("for_generate_statement", "generate_label"),
            generate_begin,
        ),
        (
            IfGenerateStatement,
            "if_generate_statement",
            ("if_generate_statement", "generate_label"),
            generate_begin,
        ),
    ] {
        r.block(kind, name, open, Some(begin), false);
    }
    r.branches(
        IfStatement,
        &[IfStatementElsif, IfStatementElse],
        "if_statement",
        ("if_statement", "if_keyword"),
        ("if_statement", "elsif_keyword"),
        false,
    );
    r.branches(
        IfGenerateStatement,
        &[IfGenerateElsif, IfGenerateElse],
        "if_generate_statement",
        ("if_generate_statement", "generate_label"),
        ("if_generate_statement", "elsif_keyword"),
        false,
    );
    r.branches(
        CaseStatement,
        &[CaseStatementAlternative],
        "case_statement",
        ("case_statement", "case_keyword"),
        ("case_statement_alternative", "when_keyword"),
        true,
    );
    r.branches(
        CaseGenerateStatement,
        &[CaseGenerateAlternative],
        "case_generate_statement",
        ("case_generate_statement", "generate_label"),
        ("case_generate_alternative", "when_keyword"),
        true,
    );
    r.list(GenericClause, "generic_clause", "generic_keyword");
    r.list(PortClause, "port_clause", "port_keyword");
    r.list(GenericMapAspect, "generic_map_aspect", "generic_keyword");
    r.list(PortMapAspect, "port_map_aspect", "port_keyword");
    let keyword_after = r.get(("assertion", "keyword"), "after");
    let report = r.get(("assertion", "report_keyword"), "token");
    if let Some(level) = r
        .apply(0, keyword_after, false, "assertion")
        .and_then(|l| r.apply(l, report, false, "assertion"))
    {
        r.set(Assertion, Part::Body, level, "assertion");
    }
    match r.get(("use_clause", "keyword"), "token_after_library_clause") {
        Setting::Relative(d) if d >= 0 => {
            let d = usize::try_from(d).unwrap_or(1);
            r.policy.use_after_library = (d != 1).then_some(d);
        }
        _ => r.warnings.push(
            "indent.tokens.use_clause: token_after_library_clause must be `current` or `+N`".into(),
        ),
    }
    let unsupported = r.unsupported();
    if !unsupported.is_empty() {
        r.warnings.push(format!(
            "indent.tokens: settings for {} are not supported and are ignored",
            unsupported.join(", ")
        ));
    }
    r.warnings.dedup();
    (r.policy, r.warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vsg_defaults_are_the_formatter_defaults() {
        let (policy, warnings) = resolve(None);
        assert_eq!(policy, IndentPolicy::default());
        assert!(warnings.is_empty(), "{warnings:?}");
        let dump = &crate::vsg_defaults::defaults()["indent"]["tokens"];
        let (policy, warnings) = resolve(Some(dump));
        assert_eq!(policy, IndentPolicy::default());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn case_when_use_clauses_and_unsupported_settings() {
        let user = serde_json::json!({
            "case_statement": {"case_keyword": {"after": "+1", "token": "current"},
                               "end_keyword": {"after": "-1", "token": "-1"}},
            "case_statement_alternative": {"when_keyword": {"after": "current", "token": "current"}},
            "use_clause": {"keyword": {"after": "current", "token": "current",
                                       "token_after_library_clause": "current",
                                       "token_if_no_matching_library_clause": "+1"}},
            "architecture_body": {"architecture_keyword": {"after": 0, "token": 0},
                                  "begin_keyword": {"after": 0, "token": 0}},
            "wait_statement": {"wait_keyword": {"after": "+3", "token": "current"}},
        });
        let (policy, warnings) = resolve(Some(&user));
        // `when` at +1 and its statements at +1 too (0 relative to `when`).
        assert_eq!(policy.level(N::CaseStatement, Part::Body), 1);
        assert_eq!(policy.level(N::CaseStatementAlternative, Part::Body), 0);
        assert_eq!(policy.level(N::CaseStatement, Part::End), 0);
        assert_eq!(policy.use_after_library, Some(0));
        assert_eq!(policy.level(N::ArchitectureBody, Part::Body), 0);
        assert_eq!(policy.level(N::ArchitectureBody, Part::Statements), 0);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("wait_statement.wait_keyword"),
            "{warnings:?}"
        );
    }
}

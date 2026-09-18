//! Blank-line policy: VSG's `blank_line` rules, applied by the formatter.
//!
//! Each rule names a place ("above the `begin` keyword", "below `end process`") and a style.
//! [`policies`] turns the enabled rules into a policy for the line break in front of individual
//! tokens. Where rules disagree, requiring a blank line wins over "no code above", which wins over
//! forbidding blank lines; places without a policy keep the blank lines of the source.

use std::collections::{BTreeMap, HashMap};

use vhdl_syntax::syntax::{NodeKind as N, SyntaxNode, SyntaxToken};
use vhdl_syntax::tokens::{Keyword as Kw, TokenKind as T};

use crate::Parsed;

/// How blank lines in front of a token are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Style {
    /// No blank line.
    NoBlank,
    /// A blank line unless a comment is directly above.
    NoCode,
    /// Exactly one blank line (above any comments attached to the token).
    Require,
}

impl Style {
    pub fn parse(text: &str) -> Option<Style> {
        Some(match text {
            "require_blank_line" | "allow_comment" => Style::Require,
            "no_blank_line" => Style::NoBlank,
            "no_code" => Style::NoCode,
            _ => return None,
        })
    }
}

/// A VSG blank-line rule.
pub struct BlankRule {
    pub info: crate::rules::RuleInfo,
    /// Style when the configuration does not set one; `None` for rules without a `style`
    /// option (they always forbid blank lines).
    pub style: Option<Style>,
    anchors: fn(&Anchors<'_>, &mut Vec<SyntaxToken>),
}

fn rule(
    id: &'static str,
    style: Option<Style>,
    enabled: bool,
    anchors: fn(&Anchors<'_>, &mut Vec<SyntaxToken>),
) -> BlankRule {
    BlankRule {
        info: crate::rules::RuleInfo::formatter(
            id,
            &["blank_line"],
            enabled,
            "Blank line policy (applied by the formatter).",
        ),
        style,
        anchors,
    }
}

const R: Option<Style> = Some(Style::Require);
const NB: Option<Style> = Some(Style::NoBlank);
const NC: Option<Style> = Some(Style::NoCode);

/// Tree queries used by the anchors.
struct Anchors<'a> {
    parsed: &'a Parsed,
    by_kind: HashMap<N, Vec<SyntaxNode>>,
}

fn is_kw(t: &SyntaxToken, k: Kw) -> bool {
    t.kind() == T::Keyword(k)
}

impl Anchors<'_> {
    fn nodes(&self, kind: N) -> &[SyntaxNode] {
        self.by_kind.get(&kind).map_or(&[], Vec::as_slice)
    }

    fn next(&self, t: &SyntaxToken) -> Option<SyntaxToken> {
        self.parsed
            .next_token(t)
            .filter(|n| n.kind() != T::Eof)
            .cloned()
    }

    fn prev(&self, t: &SyntaxToken) -> Option<SyntaxToken> {
        self.parsed.prev_token(t).cloned()
    }

    /// First tokens of all nodes of `kind`.
    fn firsts(&self, kind: N, out: &mut Vec<SyntaxToken>) {
        out.extend(self.nodes(kind).iter().map(SyntaxNode::first_token));
    }

    /// Tokens following all nodes of `kind`.
    fn afters(&self, kind: N, out: &mut Vec<SyntaxToken>) {
        out.extend(
            self.nodes(kind)
                .iter()
                .filter_map(|n| self.next(&n.last_token())),
        );
    }

    /// First tokens of `child` nodes directly inside nodes of `parent`.
    fn child_firsts(&self, parent: N, child: N, out: &mut Vec<SyntaxToken>) {
        for p in self.nodes(parent) {
            out.extend(
                p.children()
                    .filter(|c| c.kind() == child)
                    .map(|c| c.first_token()),
            );
        }
    }

    /// Tokens following `child` nodes directly inside nodes of `parent`.
    fn child_afters(&self, parent: N, child: N, out: &mut Vec<SyntaxToken>) {
        for p in self.nodes(parent) {
            for c in p.children().filter(|c| c.kind() == child) {
                out.extend(self.next(&c.last_token()));
            }
        }
    }

    /// The `begin` of a node with a declarative part.
    fn separator(n: &SyntaxNode, out: &mut Vec<SyntaxToken>) {
        out.extend(
            n.children()
                .filter(|c| c.kind() == N::DeclarationStatementSeparator)
                .map(|c| c.first_token()),
        );
    }

    /// Subprogram bodies and declarations whose specification has the given kind.
    fn subprograms(&self, kind: N, out: &mut Vec<SyntaxToken>) {
        for n in self.nodes(N::SubprogramBody) {
            let spec = n
                .first_child()
                .filter(|c| c.kind() == N::SubprogramBodyPreamble)
                .and_then(|p| p.first_child());
            if spec.is_some_and(|s| s.kind() == kind) {
                out.push(n.first_token());
            }
        }
        for n in self.nodes(N::SubprogramDeclaration) {
            if n.first_child().is_some_and(|s| s.kind() == kind) {
                out.push(n.first_token());
            }
        }
    }

    /// Tokens after declarations of `kind` that are not followed by another one.
    fn after_group(&self, kind: N, out: &mut Vec<SyntaxToken>) {
        for n in self.nodes(kind) {
            if n.next_sibling().is_none_or(|s| s.kind() != kind) {
                out.extend(self.next(&n.last_token()));
            }
        }
    }

    fn generate(&self, first: bool, out: &mut Vec<SyntaxToken>) {
        for kind in [
            N::ForGenerateStatement,
            N::IfGenerateStatement,
            N::CaseGenerateStatement,
        ] {
            if first {
                self.firsts(kind, out);
            } else {
                self.afters(kind, out);
            }
        }
    }

    /// Tokens after a keyword that is a direct child of nodes of `kind`.
    fn after_keyword(&self, kind: N, keyword: Kw, out: &mut Vec<SyntaxToken>) {
        for n in self.nodes(kind) {
            let found = n
                .children_with_tokens()
                .find_map(|c| c.as_token().filter(|t| is_kw(t, keyword)));
            out.extend(found.and_then(|t| self.next(&t)));
        }
    }

    /// First tokens of the first child of the given kind inside nodes of `parent`.
    fn first_of_child(&self, parent: N, child: N, out: &mut Vec<SyntaxToken>) {
        for p in self.nodes(parent) {
            out.extend(
                p.children()
                    .find(|c| c.kind() == child)
                    .map(|c| c.first_token()),
            );
        }
    }
}

/// All blank-line rules vsg-rs applies, sorted by id.
#[allow(clippy::too_many_lines)]
pub fn rules() -> &'static [BlankRule] {
    use N::*;
    static RULES: std::sync::OnceLock<Vec<BlankRule>> = std::sync::OnceLock::new();
    RULES.get_or_init(|| {
        vec![
            rule("architecture_003", R, true, |a, o| {
                a.firsts(ArchitectureBody, o);
            }),
            rule("architecture_015", R, true, |a, o| {
                a.afters(ArchitecturePreamble, o);
            }),
            rule("architecture_016", R, true, |a, o| {
                a.child_firsts(ArchitectureBody, DeclarationStatementSeparator, o);
            }),
            rule("architecture_017", R, true, |a, o| {
                a.child_afters(ArchitectureBody, DeclarationStatementSeparator, o);
            }),
            rule("architecture_018", R, true, |a, o| {
                a.firsts(ArchitectureEpilogue, o);
            }),
            rule("architecture_200", R, true, |a, o| {
                a.afters(ArchitectureBody, o);
            }),
            rule("block_200", R, true, |a, o| a.firsts(BlockStatement, o)),
            rule("block_201", R, true, |a, o| {
                for p in a.nodes(BlockPreamble) {
                    o.extend(a.next(&p.last_token()).filter(|t| !is_kw(t, Kw::Begin)));
                }
            }),
            rule("block_202", R, true, |a, o| {
                for b in a.nodes(BlockStatement) {
                    let header_end = b
                        .children()
                        .find(|c| c.kind() == BlockPreamble)
                        .map(|p| p.last_token().text_offset());
                    let mut begins = Vec::new();
                    Anchors::separator(b, &mut begins);
                    for begin in begins {
                        if a.prev(&begin).map(|p| p.text_offset()) != header_end {
                            o.push(begin);
                        }
                    }
                }
            }),
            rule("block_203", R, true, |a, o| {
                a.child_afters(BlockStatement, DeclarationStatementSeparator, o);
            }),
            rule("block_204", R, true, |a, o| a.firsts(BlockEpilogue, o)),
            rule("block_205", R, true, |a, o| a.afters(BlockStatement, o)),
            rule("case_007", NC, true, |a, o| a.firsts(CaseStatement, o)),
            rule("case_009", R, true, |a, o| {
                a.firsts(CaseStatementEpilogue, o);
            }),
            rule("case_010", R, true, |a, o| a.afters(CaseStatement, o)),
            rule("case_200", R, true, |a, o| {
                a.afters(CaseStatementAlternativePreamble, o);
            }),
            rule("case_201", R, true, |a, o| {
                a.firsts(CaseStatementAlternative, o);
            }),
            rule("component_003", NC, true, |a, o| {
                a.firsts(ComponentDeclaration, o);
            }),
            rule("component_016", NB, true, |a, o| {
                a.firsts(ComponentDeclarationEpilogue, o);
            }),
            rule("component_018", R, true, |a, o| {
                a.afters(ComponentDeclaration, o);
            }),
            rule("constant_200", R, false, |a, o| {
                a.after_group(ConstantDeclaration, o);
            }),
            rule("context_003", NC, true, |a, o| {
                a.firsts(ContextDeclaration, o);
            }),
            rule("context_023", R, true, |a, o| {
                a.afters(ContextDeclarationPreamble, o);
            }),
            rule("context_024", NC, true, |a, o| {
                a.firsts(ContextDeclarationEpilogue, o);
            }),
            rule("context_025", R, true, |a, o| {
                a.afters(ContextDeclaration, o);
            }),
            rule("entity_003", R, true, |a, o| a.firsts(EntityDeclaration, o)),
            rule("entity_016", NB, true, |a, o| {
                a.firsts(EntityDeclarationEpilogue, o);
            }),
            rule("entity_200", NB, true, |a, o| {
                a.child_firsts(EntityHeader, GenericClause, o);
            }),
            rule("entity_201", None, true, |a, o| {
                a.afters(EntityDeclarationPreamble, o);
            }),
            rule("entity_202", NB, true, |a, o| {
                a.child_firsts(EntityHeader, PortClause, o);
            }),
            rule("entity_203", R, true, |a, o| a.afters(EntityDeclaration, o)),
            rule("function_006", R, true, |a, o| {
                a.subprograms(FunctionSpecification, o);
            }),
            rule("generate_003", R, true, |a, o| a.generate(false, o)),
            rule("generate_004", R, true, |a, o| a.generate(true, o)),
            rule("if_006", None, true, |a, o| {
                a.afters(IfStatementPreamble, o);
                a.after_keyword(IfStatementElsif, Kw::Then, o);
            }),
            rule("if_007", None, true, |a, o| a.firsts(IfStatementElsif, o)),
            rule("if_008", None, true, |a, o| {
                a.firsts(IfStatementEpilogue, o);
            }),
            rule("if_010", None, true, |a, o| a.firsts(IfStatementElse, o)),
            rule("if_011", None, true, |a, o| {
                a.after_keyword(IfStatementElse, Kw::Else, o);
            }),
            rule("if_030", R, true, |a, o| {
                for n in a.nodes(IfStatement) {
                    let next = a.next(&n.last_token());
                    let closes = next.as_ref().is_some_and(|t| {
                        matches!(
                            t.kind(),
                            T::Keyword(Kw::End | Kw::Elsif | Kw::Else | Kw::When)
                        )
                    });
                    if !closes {
                        o.extend(next);
                    }
                }
            }),
            rule("if_031", NC, true, |a, o| {
                for n in a.nodes(IfStatement) {
                    let first = n.first_token();
                    let nested = a
                        .prev(&first)
                        .is_some_and(|p| is_kw(&p, Kw::Then) || is_kw(&p, Kw::Else));
                    if !nested {
                        o.push(first);
                    }
                }
            }),
            rule("instantiation_004", NC, true, |a, o| {
                a.firsts(ComponentInstantiationStatement, o);
            }),
            rule("instantiation_019", R, true, |a, o| {
                a.afters(ComponentInstantiationStatement, o);
            }),
            rule("library_003", R, true, |a, o| {
                // `allow_library_clause` is applied in `policies`.
                for n in a.nodes(LibraryClause) {
                    let first = n.first_token();
                    if a.prev(&first).is_some() {
                        o.push(first);
                    }
                }
            }),
            rule("library_007", NB, true, |a, o| {
                a.firsts(UseClauseContextItem, o);
            }),
            rule("loop_statement_200", NC, true, |a, o| {
                a.firsts(LoopStatement, o);
            }),
            rule("loop_statement_201", R, true, |a, o| {
                a.afters(LoopStatementPreamble, o);
            }),
            rule("loop_statement_202", R, true, |a, o| {
                a.firsts(LoopStatementEpilogue, o);
            }),
            rule("loop_statement_203", R, true, |a, o| {
                a.afters(LoopStatement, o);
            }),
            rule("package_003", NC, true, |a, o| {
                a.firsts(PackageDeclaration, o);
            }),
            rule("package_011", R, true, |a, o| a.afters(PackagePreamble, o)),
            rule("package_012", R, true, |a, o| a.firsts(PackageEpilogue, o)),
            rule("package_body_200", R, true, |a, o| a.firsts(PackageBody, o)),
            rule("package_body_201", R, true, |a, o| {
                a.afters(PackageBodyPreamble, o);
            }),
            rule("package_body_202", R, true, |a, o| {
                a.firsts(PackageBodyEpilogue, o);
            }),
            rule("package_body_203", R, true, |a, o| a.afters(PackageBody, o)),
            rule("package_instantiation_200", NC, true, |a, o| {
                a.firsts(PackageInstantiationDeclaration, o);
            }),
            rule("package_instantiation_201", NB, true, |a, o| {
                a.afters(PackageInstantiationDeclaration, o);
            }),
            rule("port_001", NB, true, |a, o| a.firsts(PortClause, o)),
            rule("port_022", None, true, |a, o| {
                a.first_of_child(PortClause, InterfaceList, o);
            }),
            rule("port_map_200", NB, true, |a, o| {
                a.first_of_child(PortMapAspect, AssociationList, o);
            }),
            rule("procedure_200", R, true, |a, o| {
                a.subprograms(ProcedureSpecification, o);
            }),
            rule("process_011", R, true, |a, o| a.afters(ProcessStatement, o)),
            rule("process_015", NC, true, |a, o| {
                a.firsts(ProcessStatement, o);
            }),
            rule("process_021", NB, true, |a, o| {
                for p in a.nodes(ProcessStatement) {
                    if p.children().all(|c| c.kind() != ProcessDeclarativePart) {
                        Anchors::separator(p, o);
                    }
                }
            }),
            rule("process_022", R, true, |a, o| {
                a.child_afters(ProcessStatement, DeclarationStatementSeparator, o);
            }),
            rule("process_023", R, true, |a, o| a.firsts(ProcessEpilogue, o)),
            rule("process_026", R, true, |a, o| {
                a.firsts(ProcessDeclarativePart, o);
            }),
            rule("process_027", R, true, |a, o| {
                for p in a.nodes(ProcessStatement) {
                    if p.children().any(|c| c.kind() == ProcessDeclarativePart) {
                        Anchors::separator(p, o);
                    }
                }
            }),
            rule("record_type_definition_200", NB, true, |a, o| {
                a.afters(RecordTypeDefinitionPreamble, o);
            }),
            rule("record_type_definition_201", NB, true, |a, o| {
                a.firsts(RecordTypeDefinitionEpilogue, o);
            }),
            rule("signal_200", R, false, |a, o| {
                a.after_group(SignalDeclaration, o);
            }),
            rule("subprogram_body_201", R, true, |a, o| {
                for p in a.nodes(SubprogramBodyPreamble) {
                    o.extend(a.next(&p.last_token()).filter(|t| !is_kw(t, Kw::Begin)));
                }
            }),
            rule("subprogram_body_202", R, true, |a, o| {
                for b in a.nodes(SubprogramBody) {
                    if b.children().any(|c| c.kind() == SubprogramDeclarativePart) {
                        Anchors::separator(b, o);
                    }
                }
            }),
            rule("subprogram_body_203", R, true, |a, o| {
                a.child_afters(SubprogramBody, DeclarationStatementSeparator, o);
            }),
            rule("subprogram_body_204", R, true, |a, o| {
                a.firsts(SubprogramBodyEpilogue, o);
            }),
            rule("subprogram_body_205", R, true, |a, o| {
                a.afters(SubprogramBody, o);
            }),
            rule("subtype_200", R, false, |a, o| {
                a.after_group(SubtypeDeclaration, o);
            }),
            rule("subtype_201", R, true, |a, o| {
                a.firsts(SubtypeDeclaration, o);
            }),
            rule("subtype_202", R, true, |a, o| {
                a.afters(SubtypeDeclaration, o);
            }),
            rule("type_010", R, true, |a, o| {
                a.firsts(FullTypeDeclaration, o);
                a.firsts(IncompleteTypeDeclaration, o);
            }),
            rule("type_011", R, true, |a, o| {
                a.afters(FullTypeDeclaration, o);
                a.afters(IncompleteTypeDeclaration, o);
            }),
            rule("type_200", R, false, |a, o| {
                a.after_group(FullTypeDeclaration, o);
            }),
        ]
    })
}

/// Blank-line settings resolved from the configuration.
#[derive(Debug, Clone)]
pub struct BlankSettings {
    /// Enabled rules and their styles (`None`: the rule has no style and forbids blank lines).
    pub rules: BTreeMap<&'static str, Option<Style>>,
    /// `library_003`: allow a library clause directly above another one.
    pub allow_library_clause: bool,
    /// `whitespace_200`: the maximum number of consecutive blank lines.
    pub max_blank_lines: usize,
    /// Pragma comment patterns (`pragma_400` to `pragma_403`), if enabled.
    pub pragmas: Option<Pragmas>,
}

#[derive(Debug, Clone)]
pub struct Pragmas {
    pub open: Vec<regex::Regex>,
    pub close: Vec<regex::Regex>,
}

impl Pragmas {
    pub fn new(open: &[&str], close: &[&str]) -> Pragmas {
        let compile = |patterns: &[&str]| {
            patterns
                .iter()
                .filter_map(|p| regex::Regex::new(p).ok())
                .collect()
        };
        Pragmas {
            open: compile(open),
            close: compile(close),
        }
    }
}

impl Default for Pragmas {
    fn default() -> Self {
        Pragmas::new(
            &[
                r"^\s*--\s+synthesis\s+translate_off\s*$",
                r"^\s*--vhdl_comp_off\s*$",
                r"^\s*--\s+RTL_SYNTHESIS\s+OFF\s*$",
            ],
            &[
                r"^\s*--\s+synthesis\s+translate_on\s*$",
                r"^\s*--vhdl_comp_on\s*$",
                r"^\s*--\s+RTL_SYNTHESIS\s+ON\s*$",
            ],
        )
    }
}

impl Default for BlankSettings {
    fn default() -> Self {
        BlankSettings {
            rules: rules()
                .iter()
                .filter(|r| r.info.enabled_by_default)
                .map(|r| (r.info.id, r.style))
                .collect(),
            allow_library_clause: false,
            max_blank_lines: 1,
            pragmas: Some(Pragmas::default()),
        }
    }
}

impl BlankSettings {
    /// No blank-line rules: blank lines of the source are kept (up to the maximum).
    pub fn preserve() -> BlankSettings {
        BlankSettings {
            rules: BTreeMap::new(),
            allow_library_clause: false,
            max_blank_lines: 1,
            pragmas: None,
        }
    }

    /// Pragma kind of a comment: `Some(true)` opens, `Some(false)` closes a pragma region.
    pub fn pragma(&self, comment: &[u8]) -> Option<bool> {
        let pragmas = self.pragmas.as_ref()?;
        let text = String::from_utf8_lossy(comment);
        if pragmas.open.iter().any(|r| r.is_match(&text)) {
            Some(true)
        } else if pragmas.close.iter().any(|r| r.is_match(&text)) {
            Some(false)
        } else {
            None
        }
    }
}

/// Policy for the line break in front of tokens, keyed by token text offset.
pub fn policies(parsed: &Parsed, settings: &BlankSettings) -> HashMap<usize, Style> {
    let mut out: HashMap<usize, Style> = HashMap::new();
    if settings.rules.is_empty() {
        return out;
    }
    let mut by_kind: HashMap<N, Vec<SyntaxNode>> = HashMap::new();
    let mut stack = vec![parsed.root().clone()];
    while let Some(n) = stack.pop() {
        stack.extend(n.children());
        by_kind.entry(n.kind()).or_default().push(n);
    }
    let anchors = Anchors { parsed, by_kind };
    let mut tokens = Vec::new();
    for rule in rules() {
        let Some(style) = settings.rules.get(rule.info.id) else {
            continue;
        };
        let style = style.unwrap_or(Style::NoBlank);
        tokens.clear();
        (rule.anchors)(&anchors, &mut tokens);
        for t in &tokens {
            if rule.info.id == "library_003" && settings.allow_library_clause {
                let after_library = anchors
                    .prev(t)
                    .is_some_and(|p| p.parent().kind() == N::LibraryClause);
                if after_library {
                    continue;
                }
            }
            let entry = out.entry(t.text_offset()).or_insert(style);
            *entry = (*entry).max(style);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str, config: &str) -> String {
        let cfg = crate::Config::parse(config).unwrap();
        String::from_utf8(crate::format(src.as_bytes().to_vec(), &cfg.format).unwrap()).unwrap()
    }

    #[test]
    fn vsg_default_blank_lines() {
        let src = "architecture a of e is\nsignal s : bit;\nbegin\nprocess begin\nnull;\nend process;\nend architecture;\n";
        assert_eq!(
            fmt(src, ""),
            "architecture a of e is\n\n  signal s : bit;\n\nbegin\n\n  process\n  begin\n\n    null;\n\n  end process;\n\nend architecture;\n"
        );
        // Without blank-line rules the source layout is kept.
        let keep = "rule: {group: {blank_line: {disable: true}}}";
        assert_eq!(
            fmt(src, keep),
            "architecture a of e is\n  signal s : bit;\nbegin\n  process\n  begin\n    null;\n  end process;\nend architecture;\n"
        );
        // Styles can be changed per rule.
        let no_blank = "rule: {architecture_015: {style: no_blank_line}}";
        assert!(fmt(src, no_blank).starts_with("architecture a of e is\n  signal"));
    }

    #[test]
    fn maximum_blank_lines_and_pragmas() {
        let src = "architecture a of e is\nbegin\n  x <= y;\n\n\n\n  y <= z;\n  -- synthesis translate_off\n\n  z <= w;\n  -- synthesis translate_on\n  w <= v;\nend architecture;\n";
        let out = fmt(src, "rule: {whitespace_200: {blank_lines_allowed: 2}}");
        assert!(out.contains("x <= y;\n\n\n  y <= z;\n\n  -- synthesis translate_off\n  z <= w;\n  -- synthesis translate_on\n\n  w <= v;"), "{out}");
    }

    #[test]
    fn rules_are_sorted_and_known() {
        assert!(rules().windows(2).all(|w| w[0].info.id < w[1].info.id));
        for r in rules() {
            assert!(crate::rules::is_known_rule(r.info.id), "{}", r.info.id);
        }
    }
}

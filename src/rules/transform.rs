//! Structural rules that remove, add or split constructs: statement labels, `component`
//! keywords, port modes, multi-identifier declarations, positional associations, comments in
//! statements, default values and clock-process conventions.

use vhdl_syntax::syntax::{NodeKind as N, SyntaxNode, SyntaxToken};
use vhdl_syntax::tokens::{Keyword as Kw, TokenKind as T, TriviaPiece};

use super::select::{all_tokens, child, children, ident_list, text, tokens};
use super::{Check, Context, Edit, Fix, FixSafety, Rule, RuleInfo, Violation, violation};
use crate::config::{RuleSettings, Severity};

fn rule(id: &'static str, enabled: bool, description: &'static str, check: Check) -> Rule {
    Rule {
        info: RuleInfo {
            id,
            groups: &["structure"],
            severity: Severity::Error,
            enabled_by_default: enabled,
            description,
        },
        check,
    }
}

fn source_text(cx: &Context<'_>, range: std::ops::Range<usize>) -> String {
    String::from_utf8_lossy(&cx.parsed.source()[range]).into_owned()
}

fn edit(start: usize, end: usize, text: impl Into<String>) -> Edit {
    Edit {
        start,
        end,
        text: text.into(),
        rank: 0,
    }
}

#[allow(clippy::unnecessary_wraps)] // Assigned to `Violation::fix`.
fn fix(safety: FixSafety, edits: Vec<Edit>) -> Option<Fix> {
    Some(Fix { safety, edits })
}

/// The end of the code before `t` (the start of its leading trivia).
fn code_end_before(cx: &Context<'_>, t: &SyntaxToken) -> usize {
    cx.parsed
        .prev_token(t)
        .map_or(t.text_offset(), |p| p.text_range().end)
}

/// Remove a node together with the whitespace in front of it.
fn delete_node(cx: &Context<'_>, n: &SyntaxNode) -> Edit {
    edit(
        code_end_before(cx, &n.first_token()),
        n.last_token().text_range().end,
        "",
    )
}

fn has_comment(t: &SyntaxToken) -> bool {
    t.leading_trivia().into_iter().any(|p| {
        matches!(
            p,
            TriviaPiece::LineComment(_) | TriviaPiece::BlockComment(_)
        )
    })
}

/// Whether any token after the first one of `n` carries a comment.
fn has_inner_comment(n: &SyntaxNode) -> bool {
    all_tokens(n).iter().skip(1).any(has_comment)
}

// Labels ------------------------------------------------------------------------------------

fn statement_label(n: &SyntaxNode) -> Option<SyntaxNode> {
    child(n, N::StmtLabel).or_else(|| {
        n.children()
            .find(|c| {
                matches!(
                    c.kind(),
                    N::CaseStatementPreamble | N::LoopStatementPreamble
                )
            })
            .and_then(|p| child(&p, N::StmtLabel))
    })
}

fn case_end_label(n: &SyntaxNode) -> Option<SyntaxToken> {
    let epilogue = child(n, N::CaseStatementEpilogue)?;
    tokens(&epilogue).find(|t| t.kind() == T::Identifier)
}

/// Whether `name` occurs anywhere in the file apart from the tokens in `own`.
fn referenced(cx: &Context<'_>, name: &str, own: &[SyntaxToken]) -> bool {
    cx.parsed.tokens().iter().any(|t| {
        t.kind() == T::Identifier && text(t).eq_ignore_ascii_case(name) && !own.contains(t)
    })
}

/// Remove the label of every `kind` statement (for case statements also the one after `end`).
fn remove_labels(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    kinds: &[N],
    applies: fn(&SyntaxNode) -> bool,
) {
    for n in kinds.iter().flat_map(|k| cx.nodes(*k)) {
        let Some(label) = statement_label(n).filter(|_| applies(n)) else {
            continue;
        };
        let name_token = label.first_token();
        let name = text(&name_token);
        let colon = label.last_token();
        let after = cx
            .parsed
            .next_token(&colon)
            .map_or(colon.text_range().end, SyntaxToken::text_offset);
        let mut own = vec![name_token.clone()];
        let mut edits = vec![edit(name_token.text_offset(), after, "")];
        if let Some(end_label) = case_end_label(n) {
            edits.push(edit(
                code_end_before(cx, &end_label),
                end_label.text_range().end,
                "",
            ));
            own.push(end_label);
        }
        // Attribute specifications may refer to the label.
        let safety = if referenced(cx, &name, &own) {
            FixSafety::Unsafe
        } else {
            FixSafety::Safe
        };
        let mut v = violation(settings, rule, &name_token, "Remove Label");
        v.fix = fix(safety, edits);
        out.push(v);
    }
}

fn case_end_labels(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    for n in cx.nodes(N::CaseStatement) {
        if let Some(t) = case_end_label(n) {
            let mut v = violation(settings, "case_020", &t, "Remove Label");
            v.fix = fix(
                FixSafety::Safe,
                vec![edit(code_end_before(cx, &t), t.text_range().end, "")],
            );
            out.push(v);
        }
    }
}

const CONCURRENT_ASSIGNMENTS: [N; 3] = [
    N::ConcurrentSimpleSignalAssignment,
    N::ConcurrentConditionalSignalAssignment,
    N::ConcurrentSelectedSignalAssignment,
];

/// `label : name(args);` is a procedure call; without arguments it may be an instantiation.
fn is_procedure_call(n: &SyntaxNode) -> bool {
    child(n, N::Name).is_some_and(|name| child(&name, N::ParenthesizedName).is_some())
}

// Declarations ------------------------------------------------------------------------------

fn has_mode(decl: &SyntaxNode) -> bool {
    tokens(decl).any(|t| {
        matches!(
            t.kind(),
            T::Keyword(Kw::In | Kw::Out | Kw::Inout | Kw::Buffer | Kw::Linkage | Kw::View)
        )
    })
}

/// `port (a : bit)` → `port (a : in bit)`.
fn port_modes(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    for list in cx
        .nodes(N::PortClause)
        .iter()
        .filter_map(|c| child(c, N::InterfaceList))
    {
        for decl in children(&list, N::InterfaceObjectDeclaration) {
            let Some(colon) = tokens(&decl).find(|t| t.kind() == T::Colon) else {
                continue;
            };
            if has_mode(&decl) {
                continue;
            }
            let at = colon.text_range().end;
            let mut v = violation(settings, "port_023", &colon, "Add mode or view");
            v.fix = fix(FixSafety::Safe, vec![edit(at, at, " in")]);
            out.push(v);
        }
    }
}

/// `signal a, b : t;` → `signal a : t; signal b : t;`. An object declaration with several
/// identifiers is equivalent to a sequence of single declarations (IEEE 1076, object declarations).
fn split_declarations(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    kind: N,
    default_limit: usize,
) {
    let limit = settings
        .option_usize("consecutive")
        .unwrap_or(default_limit)
        .max(1);
    let decls: Vec<SyntaxNode> = if kind == N::InterfaceObjectDeclaration {
        cx.nodes(N::PortClause)
            .iter()
            .filter_map(|c| child(c, N::InterfaceList))
            .flat_map(|l| children(&l, kind).collect::<Vec<_>>())
            .collect()
    } else {
        cx.nodes(kind).to_vec()
    };
    for decl in decls {
        let ids = ident_list(&decl);
        let (Some(list), true) = (child(&decl, N::IdentifierList), ids.len() > limit) else {
            continue;
        };
        let mut v = violation(settings, rule, &ids[1], split_message(rule, &ids));
        // Comments inside the declaration would be lost or duplicated.
        if !has_inner_comment(&decl) {
            let range = decl.text_range();
            let prefix = source_text(cx, range.start..ids[0].text_offset());
            let terminated = decl.last_token().kind() == T::SemiColon;
            let tail_end = if terminated {
                decl.last_token().text_offset()
            } else {
                range.end
            };
            let mut tail = source_text(cx, list.last_token().text_range().end..tail_end);
            // Include port_023's missing `in`, whose edit this replacement overlaps.
            if kind == N::InterfaceObjectDeclaration
                && enabled(cx, "port_023").is_some()
                && !has_mode(&decl)
            {
                tail = tail.replacen(':', ": in", 1);
            }
            let mut replacement = ids
                .iter()
                .map(|id| format!("{prefix}{}{tail}", text(id)))
                .collect::<Vec<_>>()
                .join("; ");
            if terminated {
                replacement.push(';');
            }
            v.fix = fix(
                FixSafety::Safe,
                vec![edit(range.start, range.end, replacement)],
            );
        }
        out.push(v);
    }
}

fn split_message(rule: &str, ids: &[SyntaxToken]) -> String {
    match rule {
        "port_026" => {
            let names: Vec<String> = ids.iter().map(text).collect();
            format!(
                "Split identifiers {} to individual lines.",
                names.join(", ")
            )
        }
        "signal_015" => "Split signal declaration into individual declarations".into(),
        _ => "Split variable declaration into individual declarations".into(),
    }
}

/// `context a, b;` → `context a; context b;`.
fn split_context_references(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    for r in cx.nodes(N::ContextReference) {
        let names: Vec<SyntaxNode> = child(r, N::NameList)
            .map(|l| children(&l, N::Name).collect())
            .unwrap_or_default();
        if names.len() < 2 {
            continue;
        }
        let mut v = violation(
            settings,
            "context_ref_009",
            &names[1].first_token(),
            "Split context references into individual references",
        );
        if !has_inner_comment(r) {
            let replacement = names
                .iter()
                .map(|n| format!("context {};", source_text(cx, n.text_range())))
                .collect::<Vec<_>>()
                .join(" ");
            let range = r.text_range();
            v.fix = fix(
                FixSafety::Safe,
                vec![edit(range.start, range.end, replacement)],
            );
        }
        out.push(v);
    }
}

/// Default values of signals and variables; removing them changes initialization.
fn default_values(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    kind: N,
) {
    for decl in cx.nodes(kind) {
        if let Some(init) = child(decl, N::InitialValue) {
            let mut v = violation(
                settings,
                rule,
                &init.first_token(),
                "Remove default assignment.",
            );
            v.fix = fix(FixSafety::Unsafe, vec![delete_node(cx, &init)]);
            out.push(v);
        }
    }
}

// Instantiations ----------------------------------------------------------------------------

fn component_keyword(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let remove = settings.option_str("action") == Some("remove");
    for inst in cx.nodes(N::InstantiatedComponent) {
        let keyword = tokens(inst).find(|t| t.kind() == T::Keyword(Kw::Component));
        match (keyword, remove) {
            (Some(k), true) => {
                let end = cx
                    .parsed
                    .next_token(&k)
                    .map_or(k.text_range().end, SyntaxToken::text_offset);
                let mut v = violation(
                    settings,
                    "instantiation_033",
                    &k,
                    "Remove *component* keyword",
                );
                v.fix = fix(FixSafety::Safe, vec![edit(k.text_offset(), end, "")]);
                out.push(v);
            }
            (None, false) => {
                let first = inst.first_token();
                let at = first.text_offset();
                let mut v = violation(
                    settings,
                    "instantiation_033",
                    &first,
                    "Add *component* keyword",
                );
                v.fix = fix(FixSafety::Safe, vec![edit(at, at, "component ")]);
                out.push(v);
            }
            _ => {}
        }
    }
}

fn instantiation_method(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let (kind, message) = if settings.option_str("method") == Some("entity") {
        (N::InstantiatedComponent, "Change to entity instantiation")
    } else {
        (N::InstantiatedEntity, "Change to component instantiation")
    };
    for inst in cx.nodes(kind) {
        out.push(violation(
            settings,
            "instantiation_034",
            &inst.first_token(),
            message,
        ));
    }
}

fn architecture_names(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let remove = settings.option_str("action") == Some("remove");
    for name in cx
        .nodes(N::InstantiatedEntity)
        .iter()
        .filter_map(|i| child(i, N::Name))
    {
        match (child(&name, N::ParenthesizedName), remove) {
            (Some(arch), true) => {
                let mut v = violation(
                    settings,
                    "instantiation_036",
                    &arch.first_token(),
                    "Remove architecture identifier",
                );
                // The binding then depends on the most recently analyzed architecture.
                v.fix = fix(FixSafety::Unsafe, vec![delete_node(cx, &arch)]);
                out.push(v);
            }
            (None, false) => out.push(violation(
                settings,
                "instantiation_036",
                &name.last_token(),
                "Add architecture identifier",
            )),
            _ => {}
        }
    }
}

fn positional(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    kind: N,
) {
    for list in cx
        .nodes(kind)
        .iter()
        .filter_map(|m| child(m, N::AssociationList))
    {
        for e in children(&list, N::AssociationElement) {
            if child(&e, N::Formal).is_none() {
                out.push(violation(
                    settings,
                    rule,
                    &e.first_token(),
                    "Add formal_part to positional assignment.",
                ));
            }
        }
    }
}

/// Comments at the end of lines inside `nodes`; removing them loses information.
fn trailing_comments(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    nodes: Vec<SyntaxNode>,
) {
    let source = cx.parsed.source();
    for n in nodes {
        // A comment after the last token is in the leading trivia of the next token.
        let after = cx.parsed.next_token(&n.last_token()).cloned();
        for t in all_tokens(&n).into_iter().skip(1).chain(after) {
            let trivia = t.leading_trivia();
            let len: usize = trivia.into_iter().map(TriviaPiece::byte_len).sum();
            let mut offset = t.text_offset() - len;
            for piece in trivia {
                if piece.is_newline() {
                    break;
                }
                if let TriviaPiece::LineComment(c) = piece {
                    let end = offset + c.as_bytes().len();
                    let start = source[..offset]
                        .iter()
                        .rposition(|&b| !matches!(b, b' ' | b'\t'))
                        .map_or(offset, |p| p + 1);
                    out.push(Violation {
                        rule,
                        severity: settings.severity,
                        start: offset,
                        end,
                        message: "Remove comment.".into(),
                        fix: fix(FixSafety::Safe, vec![edit(start, end, "")]),
                    });
                }
                offset += piece.byte_len();
            }
        }
    }
}

fn children_of(cx: &Context<'_>, parent: N, kinds: [N; 2]) -> Vec<SyntaxNode> {
    cx.nodes(parent)
        .iter()
        .flat_map(|p| p.children().filter(|c| kinds.contains(&c.kind())))
        .collect()
}

const SEQUENTIAL_SIGNAL_ASSIGNMENTS: [N; 7] = [
    N::SimpleWaveformAssignment,
    N::ConditionalWaveformAssignment,
    N::SelectedWaveformAssignment,
    N::SimpleForceAssignment,
    N::ConditionalForceAssignment,
    N::SelectedForceAssignment,
    N::SimpleReleaseAssignment,
];

const VARIABLE_ASSIGNMENTS: [N; 3] = [
    N::SimpleVariableAssignment,
    N::ConditionalVariableAssignment,
    N::SelectedVariableAssignment,
];

fn comments_in_statements(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    kinds: &[N],
) {
    for n in kinds.iter().flat_map(|k| cx.nodes(*k)) {
        if let Some(t) = all_tokens(n).iter().skip(1).find(|t| has_comment(t)) {
            let what = if rule == "sequential_006" {
                "sequential"
            } else {
                "variable"
            };
            out.push(violation(
                settings,
                rule,
                t,
                format!("Remove comments inside {what} assignment"),
            ));
        }
    }
}

// Clock processes ---------------------------------------------------------------------------

/// Tokens of a condition without enclosing parentheses, with their lower-cased texts.
fn condition_tokens(n: &SyntaxNode) -> (Vec<String>, Vec<SyntaxToken>) {
    let mut toks = all_tokens(n);
    if n.kind() == N::ParenthesizedExpressionOrAggregate && toks.len() >= 2 {
        toks = toks[1..toks.len() - 1].to_vec();
    }
    let words = toks.iter().map(|t| text(t).to_ascii_lowercase()).collect();
    (words, toks)
}

/// A clock condition: `rising_edge(c)`/`falling_edge(c)` (`edge`) or
/// `c'event and c = '1'` in either order (event form).
struct Clock {
    signal: usize,
    rising: bool,
    edge: bool,
}

fn clock(words: &[String]) -> Option<Clock> {
    let w: Vec<&str> = words.iter().map(String::as_str).collect();
    match w.as_slice() {
        [f @ ("rising_edge" | "falling_edge"), "(", _, ")"] => Some(Clock {
            signal: 2,
            rising: *f == "rising_edge",
            edge: true,
        }),
        [a, "'", "event", "and", b, "=", v] if a == b && matches!(*v, "'1'" | "'0'") => {
            Some(Clock {
                signal: 0,
                rising: *v == "'1'",
                edge: false,
            })
        }
        [b, "=", v, "and", a, "'", "event"] if a == b && matches!(*v, "'1'" | "'0'") => {
            Some(Clock {
                signal: 4,
                rising: *v == "'1'",
                edge: false,
            })
        }
        _ => None,
    }
}

/// The condition of an `if` statement (its first branch) or of an `elsif` branch.
fn condition(branch: &SyntaxNode) -> Option<SyntaxNode> {
    let n = if branch.kind() == N::IfStatement {
        child(branch, N::IfStatementPreamble)?
    } else {
        branch.clone()
    };
    n.children()
        .find(|c| !matches!(c.kind(), N::StmtLabel | N::SequenceOfStatements))
}

/// Settings of another rule, if it is enabled.
fn enabled(cx: &Context<'_>, rule: &str) -> Option<RuleSettings> {
    super::info(rule)
        .map(|info| cx.config.rule(info))
        .filter(|s| s.enabled && s.fixable)
}

/// Each branch of an `if` statement with a condition, with its statements.
fn branches(ifs: &SyntaxNode) -> Vec<(SyntaxNode, Option<SyntaxNode>)> {
    let mut out = vec![(ifs.clone(), child(ifs, N::SequenceOfStatements))];
    out.extend(children(ifs, N::IfStatementElsif).map(|e| {
        let statements = child(&e, N::SequenceOfStatements);
        (e, statements)
    }));
    out
}

fn process_ifs<'a>(cx: &'a Context<'_>) -> impl Iterator<Item = &'a SyntaxNode> {
    cx.nodes(N::IfStatement)
        .iter()
        .filter(|n| n.ancestors().any(|a| a.kind() == N::ProcessStatement))
}

fn descendants(n: &SyntaxNode, kind: N) -> Vec<SyntaxNode> {
    let mut out = Vec::new();
    let mut stack = vec![n.clone()];
    while let Some(x) = stack.pop() {
        if x.kind() == kind {
            out.push(x.clone());
        }
        stack.extend(x.children());
    }
    out.sort_by_key(SyntaxNode::offset);
    out
}

/// Statement blocks of clock processes: the reset branches before the clock branch
/// (`false`) and the clock branch (`true`).
fn clock_blocks(cx: &Context<'_>) -> Vec<(bool, SyntaxNode)> {
    let mut out = Vec::new();
    for ifs in process_ifs(cx) {
        let all = branches(ifs);
        let Some(clock_at) = all.iter().position(|(b, _)| {
            condition(b).is_some_and(|c| clock(&condition_tokens(&c).0).is_some())
        }) else {
            continue;
        };
        for (i, (_, statements)) in all.into_iter().enumerate().take(clock_at + 1) {
            if let Some(s) = statements {
                out.push((i == clock_at, s));
            }
        }
    }
    out
}

fn missing_after(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let magnitude = settings.option_usize("magnitude").unwrap_or(1);
    let units = settings.option_str("units").unwrap_or("ns");
    for (_, block) in clock_blocks(cx).into_iter().filter(|(c, _)| *c) {
        for a in descendants(&block, N::SimpleWaveformAssignment) {
            let elements: Vec<SyntaxNode> = child(&a, N::WaveformElements)
                .map(|e| children(&e, N::WaveformElement).collect())
                .unwrap_or_default();
            let [element] = elements.as_slice() else {
                continue;
            };
            if child(element, N::AfterClause).is_some() {
                continue;
            }
            let end = element.last_token().text_range().end;
            let mut v = violation(
                settings,
                "after_001",
                &a.first_token(),
                format!("Add after {magnitude} {units} to signal in clock process"),
            );
            v.fix = fix(
                FixSafety::Safe,
                vec![edit(end, end, format!(" after {magnitude} {units}"))],
            );
            out.push(v);
        }
    }
}

fn reset_after(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    for (_, block) in clock_blocks(cx).into_iter().filter(|(c, _)| !*c) {
        for after in descendants(&block, N::AfterClause) {
            let mut v = violation(
                settings,
                "after_003",
                &after.first_token(),
                "Remove *after* from signals in reset portion of a clock process",
            );
            v.fix = fix(FixSafety::Safe, vec![delete_node(cx, &after)]);
            out.push(v);
        }
    }
}

fn clock_style(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let want_edge = settings.option_str("clock") == Some("edge");
    for ifs in process_ifs(cx) {
        for cond in branches(ifs).iter().filter_map(|(b, _)| condition(b)) {
            let (words, toks) = condition_tokens(&cond);
            let Some(c) = clock(&words).filter(|c| c.edge != want_edge) else {
                continue;
            };
            let signal = text(&toks[c.signal]);
            let replacement = if want_edge {
                let f = if c.rising {
                    "rising_edge"
                } else {
                    "falling_edge"
                };
                format!("{f}({signal})")
            } else {
                let value = if c.rising { "'1'" } else { "'0'" };
                format!("{signal}'event and {signal} = {value}")
            };
            // A function call counts as parenthesized for if_002; the event form does not.
            let replacement = if !want_edge
                && cond.kind() != N::ParenthesizedExpressionOrAggregate
                && enabled(cx, "if_002")
                    .is_some_and(|s| s.option_str("parenthesis") != Some("remove"))
            {
                format!("({replacement})")
            } else {
                replacement
            };
            let (first, last) = (&toks[0], &toks[toks.len() - 1]);
            let mut v = violation(
                settings,
                "process_029",
                first,
                if want_edge {
                    "Change event to rising_edge format.".to_owned()
                } else if c.rising {
                    "Change rising_edge to event format.".to_owned()
                } else {
                    "Change falling_edge to event format.".to_owned()
                },
            );
            // The forms differ for transitions from and to metavalues ('X', 'H', 'L'); VSG
            // applies this fix when the rule is enabled, so vsg-rs does too.
            v.fix = fix(
                FixSafety::Safe,
                vec![edit(
                    first.text_offset(),
                    last.text_range().end,
                    replacement,
                )],
            );
            out.push(v);
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn rules() -> Vec<Rule> {
    vec![
        rule(
            "case_019",
            true,
            "Case statements have no label.",
            |cx, s, out| remove_labels(cx, s, out, "case_019", &[N::CaseStatement], |_| true),
        ),
        rule(
            "case_020",
            true,
            "`end case` has no label.",
            case_end_labels,
        ),
        rule(
            "concurrent_005",
            true,
            "Concurrent signal assignments have no label.",
            |cx, s, out| {
                remove_labels(
                    cx,
                    s,
                    out,
                    "concurrent_005",
                    &CONCURRENT_ASSIGNMENTS,
                    |_| true,
                );
            },
        ),
        rule(
            "procedure_call_001",
            true,
            "Procedure calls have no label.",
            |cx, s, out| {
                remove_labels(
                    cx,
                    s,
                    out,
                    "procedure_call_001",
                    &[N::ProcedureCallStatement],
                    |_| true,
                );
            },
        ),
        rule(
            "procedure_call_002",
            true,
            "Concurrent procedure calls have no label.",
            |cx, s, out| {
                remove_labels(
                    cx,
                    s,
                    out,
                    "procedure_call_002",
                    &[N::ConcurrentProcedureCallOrComponentInstantiationStatement],
                    is_procedure_call,
                );
            },
        ),
        rule(
            "report_statement_001",
            true,
            "Report statements have no label.",
            |cx, s, out| {
                remove_labels(
                    cx,
                    s,
                    out,
                    "report_statement_001",
                    &[N::ReportStatement],
                    |_| true,
                );
            },
        ),
        rule(
            "port_023",
            true,
            "Port declarations have a mode.",
            port_modes,
        ),
        rule(
            "port_026",
            true,
            "Port declarations declare one port each.",
            |cx, s, out| {
                split_declarations(cx, s, out, "port_026", N::InterfaceObjectDeclaration, 1);
            },
        ),
        rule(
            "signal_015",
            true,
            "Signal declarations declare at most `consecutive` signals.",
            |cx, s, out| split_declarations(cx, s, out, "signal_015", N::SignalDeclaration, 2),
        ),
        rule(
            "variable_015",
            true,
            "Variable declarations declare at most `consecutive` variables.",
            |cx, s, out| {
                split_declarations(cx, s, out, "variable_015", N::VariableDeclaration, 2);
            },
        ),
        rule(
            "context_ref_009",
            true,
            "Context references name one context each.",
            split_context_references,
        ),
        rule(
            "signal_007",
            true,
            "Signal declarations have no default value.",
            |cx, s, out| default_values(cx, s, out, "signal_007", N::SignalDeclaration),
        ),
        rule(
            "variable_007",
            true,
            "Variable declarations have no default value.",
            |cx, s, out| default_values(cx, s, out, "variable_007", N::VariableDeclaration),
        ),
        rule(
            "instantiation_033",
            true,
            "Component instantiations include `component` (`action: remove` for the opposite).",
            component_keyword,
        ),
        rule(
            "instantiation_034",
            true,
            "Instantiations use the configured `method` (component or entity).",
            instantiation_method,
        ),
        rule(
            "instantiation_036",
            true,
            "Entity instantiations name the architecture (`action: remove` for the opposite).",
            architecture_names,
        ),
        rule(
            "generic_map_008",
            true,
            "Generic maps use named association.",
            |cx, s, out| positional(cx, s, out, "generic_map_008", N::GenericMapAspect),
        ),
        rule(
            "port_map_008",
            true,
            "Port maps use named association.",
            |cx, s, out| positional(cx, s, out, "port_map_008", N::PortMapAspect),
        ),
        rule(
            "component_019",
            true,
            "Component port and generic clauses have no trailing comments.",
            |cx, s, out| {
                let nodes = children_of(
                    cx,
                    N::ComponentDeclaration,
                    [N::PortClause, N::GenericClause],
                );
                trailing_comments(cx, s, out, "component_019", nodes);
            },
        ),
        rule(
            "port_map_010",
            true,
            "Port and generic maps have no trailing comments.",
            |cx, s, out| {
                let nodes = children_of(
                    cx,
                    N::ComponentInstantiationStatement,
                    [N::PortMapAspect, N::GenericMapAspect],
                );
                trailing_comments(cx, s, out, "port_map_010", nodes);
            },
        ),
        rule(
            "sequential_006",
            true,
            "Sequential signal assignments contain no comments.",
            |cx, s, out| {
                comments_in_statements(
                    cx,
                    s,
                    out,
                    "sequential_006",
                    &SEQUENTIAL_SIGNAL_ASSIGNMENTS,
                );
            },
        ),
        rule(
            "variable_assignment_006",
            true,
            "Variable assignments contain no comments.",
            |cx, s, out| {
                comments_in_statements(
                    cx,
                    s,
                    out,
                    "variable_assignment_006",
                    &VARIABLE_ASSIGNMENTS,
                );
            },
        ),
        rule(
            "after_001",
            false,
            "Assignments in the clock branch of clock processes have an `after` delay.",
            missing_after,
        ),
        rule(
            "after_003",
            false,
            "Assignments in the reset branch of clock processes have no `after` delay.",
            reset_after,
        ),
        rule(
            "process_029",
            false,
            "Clock conditions use the configured `clock` style (`event` or `edge`).",
            clock_style,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use crate::Parsed;
    use crate::config::Config;

    fn fixed(src: &str, yaml: &str, unsafe_fixes: bool) -> String {
        let config = Config::parse(yaml).unwrap();
        let parsed = Parsed::new(src.as_bytes().to_vec());
        let options = crate::fix::FixOptions {
            unsafe_fixes,
            ..Default::default()
        };
        let out = crate::fix::fix_with(&parsed, &config, &options).unwrap();
        String::from_utf8(out.output).unwrap()
    }

    fn rules(src: &str, yaml: &str) -> Vec<&'static str> {
        let config = Config::parse(yaml).unwrap();
        crate::rules::check(&Parsed::new(src.as_bytes().to_vec()), &config)
            .into_iter()
            .map(|v| v.rule)
            .collect()
    }

    const ARCH: &str = "architecture rtl of e is\n  signal a, b, c : bit;\nbegin\n  ca : a <= b;\n  u1 : comp port map (b, c);\n  u2 : entity work.e;\n  pc : proc(a);\n  process (clk) is\n  begin\n    if rst = '1' then\n      a <= '0' after 1 ns;\n    elsif rising_edge(clk) then\n      a <= b;\n    end if;\n    cl : case a is\n      when others => null;\n    end case cl;\n    rl : report \"x\";\n  end process;\nend architecture rtl;\n";

    #[test]
    fn safe_structural_fixes() {
        let out = fixed(ARCH, "", false);
        for expected in [
            "signal a : bit;\n  signal b : bit;\n  signal c : bit;",
            "\n  a <= b;",
            "u1 : component comp",
            "\n  proc(a);",
            "\n    case a is",
            "end case;",
            "\n    report \"x\";",
            "a <= '0' after 1 ns;",
        ] {
            assert!(out.contains(expected), "missing {expected:?} in\n{out}");
        }
    }

    #[test]
    fn unsafe_and_lint_rules() {
        let yaml = "rule:\n  after_001:\n    disable: false\n  after_003:\n    disable: false\n  process_029:\n    disable: false\n";
        let found = rules(ARCH, yaml);
        for r in [
            "port_map_008",
            "instantiation_036",
            "after_001",
            "after_003",
            "process_029",
        ] {
            assert!(found.contains(&r), "{r} missing in {found:?}");
        }
        let out = fixed(ARCH, yaml, true);
        for expected in [
            "a <= '0';",
            "a <= b after 1 ns;",
            "elsif (clk'event and clk = '1') then",
        ] {
            assert!(out.contains(expected), "missing {expected:?} in\n{out}");
        }
    }

    #[test]
    fn port_rules() {
        let src = "entity e is\n  port (\n    a, b : bit; -- keep\n    c : out bit\n  );\nend entity e;\n";
        // Adding the mode (port_023) is not a VSG default fix.
        assert_eq!(
            fixed(src, "", false),
            "entity e is\n  port (\n    a : bit;\n    b : bit; -- keep\n    c : out   bit\n  );\nend entity e;\n"
        );
        assert_eq!(
            fixed(src, "", true),
            "entity e is\n  port (\n    a : in    bit;\n    b : in    bit; -- keep\n    c : out   bit\n  );\nend entity e;\n"
        );
    }

    #[test]
    fn referenced_labels_are_kept() {
        let src = "architecture rtl of e is\n  attribute keep : boolean;\n  attribute keep of ca : label is true;\nbegin\n  ca : a <= b;\nend architecture rtl;\n";
        assert!(fixed(src, "", false).contains("  ca : a <= b;"));
        assert!(fixed(src, "", true).contains("\n  a <= b;"));
    }
}

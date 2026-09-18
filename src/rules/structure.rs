//! Structural rules with syntax-local fixes: optional closing keywords and names, optional `is`,
//! labels, parenthesized conditions and port defaults.

use vhdl_syntax::syntax::{NodeKind as N, SyntaxNode, SyntaxToken};
use vhdl_syntax::tokens::{Keyword as Kw, TokenKind as T};

use super::select::{all_tokens, label, text};
use super::{Context, Edit, Fix, FixSafety, Rule, RuleInfo, Violation, violation};
use crate::config::{RuleSettings, Severity};

type Check = fn(&Context<'_>, &RuleSettings, &mut Vec<Violation>);

/// A rule in the `structure` and `structure::optional` groups, enabled by default.
fn optional(id: &'static str, description: &'static str, check: Check) -> Rule {
    Rule {
        info: RuleInfo {
            id,
            groups: &["structure", "structure::optional"],
            severity: Severity::Error,
            enabled_by_default: true,
            description,
        },
        check,
    }
}

/// A rule in the `structure` group only.
fn structure(id: &'static str, description: &'static str, enabled: bool, check: Check) -> Rule {
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

pub(super) fn rules() -> Vec<Rule> {
    let mut loop_label_rule = optional(
        "loop_statement_007",
        "`end loop` repeats the loop label.",
        |cx, s, out| {
            closing_name(cx, s, out, "loop_statement_007", N::LoopStatement, label);
        },
    );
    loop_label_rule.info.enabled_by_default = false;
    vec![
        optional(
            "entity_015",
            "`end` of an entity includes the `entity` keyword.",
            |cx, s, out| {
                closing_keywords(
                    cx,
                    s,
                    out,
                    "entity_015",
                    N::EntityDeclaration,
                    &[Kw::Entity],
                );
            },
        ),
        optional(
            "entity_019",
            "`end` of an entity repeats the entity name.",
            |cx, s, out| {
                closing_name(
                    cx,
                    s,
                    out,
                    "entity_019",
                    N::EntityDeclaration,
                    preamble_identifier,
                );
            },
        ),
        optional(
            "architecture_010",
            "`end` of an architecture includes the `architecture` keyword.",
            |cx, s, out| {
                closing_keywords(
                    cx,
                    s,
                    out,
                    "architecture_010",
                    N::ArchitectureBody,
                    &[Kw::Architecture],
                );
            },
        ),
        optional(
            "architecture_024",
            "`end` of an architecture repeats the architecture name.",
            |cx, s, out| {
                closing_name(
                    cx,
                    s,
                    out,
                    "architecture_024",
                    N::ArchitectureBody,
                    preamble_identifier,
                );
            },
        ),
        optional(
            "package_007",
            "`end` of a package includes the `package` keyword.",
            |cx, s, out| {
                closing_keywords(
                    cx,
                    s,
                    out,
                    "package_007",
                    N::PackageDeclaration,
                    &[Kw::Package],
                );
            },
        ),
        optional(
            "package_014",
            "`end` of a package repeats the package name.",
            |cx, s, out| {
                closing_name(
                    cx,
                    s,
                    out,
                    "package_014",
                    N::PackageDeclaration,
                    preamble_identifier,
                );
            },
        ),
        optional(
            "package_body_002",
            "`end` of a package body includes `package body`.",
            |cx, s, out| {
                closing_keywords(
                    cx,
                    s,
                    out,
                    "package_body_002",
                    N::PackageBody,
                    &[Kw::Package, Kw::Body],
                );
            },
        ),
        optional(
            "package_body_003",
            "`end` of a package body repeats the package name.",
            |cx, s, out| {
                closing_name(
                    cx,
                    s,
                    out,
                    "package_body_003",
                    N::PackageBody,
                    preamble_identifier,
                );
            },
        ),
        optional(
            "component_021",
            "Component declarations include the optional `is`.",
            |cx, s, out| {
                optional_is(cx, s, out, "component_021", N::ComponentDeclaration);
            },
        ),
        optional(
            "component_022",
            "`end component` repeats the component name.",
            |cx, s, out| {
                closing_name(
                    cx,
                    s,
                    out,
                    "component_022",
                    N::ComponentDeclaration,
                    preamble_identifier,
                );
            },
        ),
        optional(
            "context_021",
            "`end` of a context includes the `context` keyword.",
            |cx, s, out| {
                closing_keywords(
                    cx,
                    s,
                    out,
                    "context_021",
                    N::ContextDeclaration,
                    &[Kw::Context],
                );
            },
        ),
        optional(
            "context_022",
            "`end` of a context repeats the context name.",
            |cx, s, out| {
                closing_name(
                    cx,
                    s,
                    out,
                    "context_022",
                    N::ContextDeclaration,
                    preamble_identifier,
                );
            },
        ),
        optional(
            "function_018",
            "`end` of a function body includes `function`.",
            |cx, s, out| {
                subprogram_keyword(
                    cx,
                    s,
                    out,
                    "function_018",
                    N::FunctionSpecification,
                    Kw::Function,
                );
            },
        ),
        optional(
            "function_020",
            "`end` of a function body repeats its designator.",
            |cx, s, out| {
                subprogram_designator(cx, s, out, "function_020", N::FunctionSpecification);
            },
        ),
        optional(
            "procedure_012",
            "`end` of a procedure body includes `procedure`.",
            |cx, s, out| {
                subprogram_keyword(
                    cx,
                    s,
                    out,
                    "procedure_012",
                    N::ProcedureSpecification,
                    Kw::Procedure,
                );
            },
        ),
        optional(
            "procedure_014",
            "`end` of a procedure body repeats its designator.",
            |cx, s, out| {
                subprogram_designator(cx, s, out, "procedure_014", N::ProcedureSpecification);
            },
        ),
        optional(
            "process_012",
            "Process statements include the optional `is`.",
            |cx, s, out| {
                optional_is(cx, s, out, "process_012", N::ProcessStatement);
            },
        ),
        optional(
            "process_018",
            "`end process` repeats the process label.",
            |cx, s, out| {
                closing_name(cx, s, out, "process_018", N::ProcessStatement, label);
            },
        ),
        optional(
            "block_002",
            "Block statements include the optional `is`.",
            |cx, s, out| {
                optional_is(cx, s, out, "block_002", N::BlockStatement);
            },
        ),
        optional(
            "block_007",
            "`end block` repeats the block label.",
            |cx, s, out| {
                closing_name(cx, s, out, "block_007", N::BlockStatement, label);
            },
        ),
        optional(
            "generate_011",
            "`end generate` repeats the generate label.",
            |cx, s, out| {
                for kind in [
                    N::ForGenerateStatement,
                    N::IfGenerateStatement,
                    N::CaseGenerateStatement,
                ] {
                    closing_name(cx, s, out, "generate_011", kind, label);
                }
            },
        ),
        loop_label_rule,
        optional(
            "record_type_definition_005",
            "`end record` repeats the record type name.",
            |cx, s, out| {
                closing_name(
                    cx,
                    s,
                    out,
                    "record_type_definition_005",
                    N::RecordTypeDefinition,
                    record_name,
                );
            },
        ),
        structure(
            "process_016",
            "Process statements have a label.",
            true,
            |cx, s, out| {
                missing_label(cx, s, out, "process_016", N::ProcessStatement, "process");
            },
        ),
        structure(
            "loop_statement_006",
            "Loop statements have a label.",
            false,
            |cx, s, out| {
                missing_label(cx, s, out, "loop_statement_006", N::LoopStatement, "loop");
            },
        ),
        structure(
            "if_002",
            "If and elsif conditions are enclosed in parentheses.",
            true,
            if_parentheses,
        ),
        structure(
            "port_012",
            "Ports have no default values.",
            true,
            port_defaults,
        ),
    ]
}

fn direct_token(n: &SyntaxNode, kind: T) -> Option<SyntaxToken> {
    n.children_with_tokens()
        .find_map(|c| c.as_token().filter(|t| t.kind() == kind))
}

fn child(n: &SyntaxNode, pred: impl Fn(N) -> bool) -> Option<SyntaxNode> {
    n.children().find(|c| pred(c.kind()))
}

fn is_epilogue(k: N) -> bool {
    matches!(
        k,
        N::EntityDeclarationEpilogue
            | N::ArchitectureEpilogue
            | N::PackageEpilogue
            | N::PackageBodyEpilogue
            | N::ComponentDeclarationEpilogue
            | N::ContextDeclarationEpilogue
            | N::SubprogramBodyEpilogue
            | N::ProcessEpilogue
            | N::BlockEpilogue
            | N::GenerateEpilogue
            | N::LoopStatementEpilogue
            | N::RecordTypeDefinitionEpilogue
    )
}

fn is_preamble(k: N) -> bool {
    matches!(
        k,
        N::EntityDeclarationPreamble
            | N::ArchitecturePreamble
            | N::PackagePreamble
            | N::PackageBodyPreamble
            | N::ComponentDeclarationPreamble
            | N::ContextDeclarationPreamble
            | N::ProcessPreamble
            | N::BlockPreamble
    )
}

/// The name declared in a construct's preamble.
fn preamble_identifier(n: &SyntaxNode) -> Option<SyntaxToken> {
    child(n, is_preamble).and_then(|p| direct_token(&p, T::Identifier))
}

fn record_name(n: &SyntaxNode) -> Option<SyntaxToken> {
    n.parent()
        .filter(|p| p.kind() == N::FullTypeDeclaration)
        .and_then(|p| direct_token(&p, T::Identifier))
}

/// VSG's solution text for adding (`add`) or removing a closing keyword or name.
fn solution(rule: &str, add: bool, arg: &str) -> String {
    if !add {
        return match rule {
            "entity_015" | "architecture_010" | "package_007" | "package_body_002"
            | "context_021" | "function_018" | "procedure_012" => {
                format!("Remove *{arg}* keyword")
            }
            _ => format!("Remove {arg}"),
        };
    }
    match rule {
        "architecture_010" => "Add architecture keyword.".into(),
        "function_018" => "Add function keyword".into(),
        "procedure_012" => "Add procedure keyword".into(),
        "package_body_002" => "Add *package body* keywords".into(),
        "entity_015" | "package_007" | "context_021" => format!("Add *{arg}* keyword"),
        "entity_019" => "Add entity simple name".into(),
        "architecture_024" => "Add architecture simple name".into(),
        "package_014" | "package_body_003" => "Add package name.".into(),
        "component_022" => "Add component_simple_name".into(),
        // Sic: VSG 3.35's text.
        "context_022" => "Add context simple same".into(),
        "function_020" => "Add function designator".into(),
        "procedure_014" => "Add procedure designator".into(),
        "generate_011" => format!("Add label {arg}"),
        "block_007" => "Add label".into(),
        "process_018" => "Add a label for the \"end process\".".into(),
        "loop_statement_007" => "Add a label for the \"end loop\".".into(),
        "record_type_definition_005" => "Add record type simple name".into(),
        _ => format!("Add {arg}"),
    }
}

fn wants_removal(settings: &RuleSettings) -> bool {
    settings.option_str("action") == Some("remove")
}

fn safe(edits: Vec<Edit>) -> Fix {
    Fix {
        safety: FixSafety::Safe,
        edits,
    }
}

fn insert(at: usize, text: String, rank: u8) -> Edit {
    Edit {
        start: at,
        end: at,
        text,
        rank,
    }
}

/// Delete a token together with the whitespace before it.
fn delete(cx: &Context<'_>, t: &SyntaxToken) -> Edit {
    let start = cx
        .parsed
        .prev_token(t)
        .map_or(t.text_offset(), |p| p.text_range().end);
    Edit {
        start,
        end: t.text_range().end,
        text: String::new(),
        rank: 0,
    }
}

fn keyword_text(k: Kw) -> String {
    format!("{k:?}").to_ascii_lowercase()
}

/// `end <keywords>`: present (or absent, with `action: remove`).
fn closing_keywords(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    kind: N,
    keywords: &[Kw],
) {
    let words: Vec<String> = keywords.iter().copied().map(keyword_text).collect();
    let words = words.join(" ");
    for n in cx.nodes(kind) {
        let Some(epilogue) = child(n, is_epilogue) else {
            continue;
        };
        let tokens = all_tokens(&epilogue);
        let Some(end) = tokens.first().filter(|t| t.kind() == T::Keyword(Kw::End)) else {
            continue;
        };
        let present: Vec<&SyntaxToken> = tokens
            .iter()
            .filter(|t| keywords.iter().any(|k| t.kind() == T::Keyword(*k)))
            .collect();
        if wants_removal(settings) {
            if let Some(first) = present.first() {
                let mut v = violation(settings, rule, first, solution(rule, false, &words));
                v.fix = Some(safe(present.iter().map(|t| delete(cx, t)).collect()));
                out.push(v);
            }
        } else if present.is_empty() {
            let mut v = violation(settings, rule, end, solution(rule, true, &words));
            v.fix = Some(safe(vec![insert(
                end.text_range().end,
                format!(" {words}"),
                0,
            )]));
            out.push(v);
        }
    }
}

/// `end ... <name>`: the closing name repeats the opening name.
fn closing_name(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    kind: N,
    name: fn(&SyntaxNode) -> Option<SyntaxToken>,
) {
    for n in cx.nodes(kind) {
        if let Some(epilogue) = child(n, is_epilogue) {
            name_in_epilogue(cx, &epilogue, name(n).as_ref(), settings, out, rule);
        }
    }
}

fn end_name_case_rule(rule: &str) -> &'static str {
    match rule {
        "entity_019" => "entity_012",
        "architecture_024" => "architecture_011",
        "package_014" => "package_008",
        "package_body_003" => "package_body_507",
        "component_022" => "component_012",
        "context_022" => "context_016",
        "function_020" => "function_506",
        "procedure_014" => "procedure_506",
        "block_007" => "block_506",
        "generate_011" => "generate_012",
        "loop_statement_007" => "loop_statement_504",
        "process_018" => "process_019",
        _ => "type_004",
    }
}

/// `name`: the opening name or label; without one, a missing closing name is still reported
/// (as VSG does) but cannot be fixed.
fn name_in_epilogue(
    cx: &Context<'_>,
    epilogue: &SyntaxNode,
    name: Option<&SyntaxToken>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
) {
    let tokens = all_tokens(epilogue);
    let closing = tokens
        .iter()
        .find(|t| matches!(t.kind(), T::Identifier | T::StringLiteral));
    match (closing, wants_removal(settings)) {
        (Some(closing), true) => {
            let mut v = violation(
                settings,
                rule,
                closing,
                solution(rule, false, &text(closing)),
            );
            v.fix = Some(safe(vec![delete(cx, closing)]));
            out.push(v);
        }
        (None, false) => {
            let Some(last) = tokens.last() else { return };
            // Before the terminating `;`, or at the end when the `;` belongs to the parent.
            let at = if last.kind() == T::SemiColon {
                last.text_offset()
            } else {
                last.text_range().end
            };
            let Some(name) = name else {
                out.push(violation(settings, rule, last, solution(rule, true, "")));
                return;
            };
            // Inserted as the end-name case rule wants it, so a second run finds nothing.
            let name = super::case::spelling(cx, end_name_case_rule(rule), text(name));
            let mut v = violation(settings, rule, last, solution(rule, true, &name));
            v.fix = Some(safe(vec![insert(at, format!(" {name}"), 1)]));
            out.push(v);
        }
        _ => {}
    }
}

fn subprogram_bodies<'c>(cx: &'c Context<'_>, spec: N) -> impl Iterator<Item = &'c SyntaxNode> {
    cx.nodes(N::SubprogramBody).iter().filter(move |b| {
        child(b, |k| k == N::SubprogramBodyPreamble)
            .and_then(|p| p.first_child())
            .is_some_and(|s| s.kind() == spec)
    })
}

/// `end function` / `end procedure`.
fn subprogram_keyword(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    spec: N,
    keyword: Kw,
) {
    let word = keyword_text(keyword);
    for body in subprogram_bodies(cx, spec) {
        let Some(epilogue) = child(body, |k| k == N::SubprogramBodyEpilogue) else {
            continue;
        };
        let tokens = all_tokens(&epilogue);
        let present = tokens.iter().find(|t| t.kind() == T::Keyword(keyword));
        match (present, wants_removal(settings)) {
            (Some(t), true) => {
                let mut v = violation(settings, rule, t, solution(rule, false, &word));
                v.fix = Some(safe(vec![delete(cx, t)]));
                out.push(v);
            }
            (None, false) => {
                let end = &tokens[0];
                let mut v = violation(settings, rule, end, solution(rule, true, &word));
                v.fix = Some(safe(vec![insert(
                    end.text_range().end,
                    format!(" {word}"),
                    0,
                )]));
                out.push(v);
            }
            _ => {}
        }
    }
}

/// `end <designator>` of subprogram bodies.
fn subprogram_designator(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    spec: N,
) {
    for body in subprogram_bodies(cx, spec) {
        let designator = child(body, |k| k == N::SubprogramBodyPreamble)
            .and_then(|p| p.first_child())
            .and_then(|s| {
                s.children_with_tokens()
                    .filter_map(|c| c.as_token())
                    .find(|t| matches!(t.kind(), T::Identifier | T::StringLiteral))
            });
        let epilogue = child(body, |k| k == N::SubprogramBodyEpilogue);
        if let (Some(epilogue), Some(designator)) = (epilogue, designator) {
            name_in_epilogue(cx, &epilogue, Some(&designator), settings, out, rule);
        }
    }
}

/// The optional `is` of process, block and component headers.
fn optional_is(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    kind: N,
) {
    for n in cx.nodes(kind) {
        let Some(preamble) = child(n, is_preamble) else {
            continue;
        };
        match (
            direct_token(&preamble, T::Keyword(Kw::Is)),
            wants_removal(settings),
        ) {
            (Some(is), true) => {
                let mut v = violation(settings, rule, &is, "Remove *is* keyword");
                v.fix = Some(safe(vec![delete(cx, &is)]));
                out.push(v);
            }
            (None, false) => {
                let last = preamble.last_token();
                // Sic: VSG 3.35 ends only this one with a period.
                let text = if rule == "component_021" {
                    "Add *is* keyword."
                } else {
                    "Add *is* keyword"
                };
                let mut v = violation(settings, rule, &last, text);
                v.fix = Some(safe(vec![insert(last.text_range().end, " is".into(), 2)]));
                out.push(v);
            }
            _ => {}
        }
    }
}

fn missing_label(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    kind: N,
    what: &str,
) {
    for n in cx.nodes(kind) {
        if label(n).is_none() {
            out.push(violation(
                settings,
                rule,
                &n.first_token(),
                format!("Add label for {what} statement"),
            ));
        }
    }
}

/// A condition counts as enclosed when it is a parenthesized expression, or a single name
/// ending in a parenthesized part (such as `rising_edge(clk)`), which matches VSG.
fn enclosed(cond: &SyntaxNode) -> bool {
    match cond.kind() {
        N::ParenthesizedExpressionOrAggregate => is_parenthesized_expression(cond),
        // As in VSG: a name with any parenthesized part (`f(x)`, `a(1).b`).
        N::NameExpression => cond
            .first_child()
            .is_some_and(|name| name.children().any(|c| c.kind() == N::ParenthesizedName)),
        _ => false,
    }
}

/// `( expression )`, as opposed to an aggregate.
fn is_parenthesized_expression(n: &SyntaxNode) -> bool {
    n.children().next().is_some_and(|list| {
        let mut elements = list.children();
        matches!((elements.next(), elements.next()), (Some(e), None)
            if e.first_child().is_none_or(|c| c.kind() != N::ElementChoices))
    })
}

fn if_parentheses(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let remove = settings.option_str("parenthesis") == Some("remove");
    for kind in [N::IfStatementPreamble, N::IfStatementElsif] {
        for n in cx.nodes(kind) {
            let Some(cond) = n
                .children()
                .find(|c| !matches!(c.kind(), N::StmtLabel | N::SequenceOfStatements))
            else {
                continue;
            };
            let parenthesized = cond.kind() == N::ParenthesizedExpressionOrAggregate
                && is_parenthesized_expression(&cond);
            let (first, last) = (cond.first_token(), cond.last_token());
            if remove && parenthesized {
                let mut v = violation(settings, "if_002", &first, "Remove ()'s from condition.");
                v.fix = Some(safe(vec![
                    Edit {
                        start: first.text_offset(),
                        end: first.text_range().end,
                        text: String::new(),
                        rank: 0,
                    },
                    Edit {
                        start: last.text_offset(),
                        end: last.text_range().end,
                        text: String::new(),
                        rank: 0,
                    },
                ]));
                out.push(v);
            } else if !remove && !enclosed(&cond) {
                let mut v = violation(settings, "if_002", &first, "Enclose condition in ()'s.");
                v.fix = Some(safe(vec![
                    insert(first.text_offset(), "(".into(), 0),
                    insert(last.text_range().end, ")".into(), 0),
                ]));
                out.push(v);
            }
        }
    }
}

/// Removing a port default changes the value of unconnected inputs, so the fix is unsafe.
fn port_defaults(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    for clause in cx.nodes(N::PortClause) {
        let mut stack = vec![clause.clone()];
        while let Some(n) = stack.pop() {
            if n.kind() != N::InitialValue {
                stack.extend(n.children());
                continue;
            }
            let assign = n.first_token();
            let start = cx
                .parsed
                .prev_token(&assign)
                .map_or(assign.text_offset(), |p| p.text_range().end);
            let mut v = violation(settings, "port_012", &assign, "Remove assignment");
            v.fix = Some(Fix {
                safety: FixSafety::Unsafe,
                edits: vec![Edit {
                    start,
                    end: n.last_token().text_range().end,
                    text: String::new(),
                    rank: 0,
                }],
            });
            out.push(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FixSafety;
    use crate::Parsed;
    use crate::config::Config;

    fn ids(src: &str, config: &str) -> Vec<&'static str> {
        let cfg = Config::parse(config).unwrap();
        crate::rules::check(&Parsed::new(src.as_bytes().to_vec()), &cfg)
            .into_iter()
            .filter(|v| v.rule != "length_001")
            .map(|v| v.rule)
            .collect()
    }

    #[test]
    fn closing_markers() {
        let src = "entity e is\nend;\narchitecture a of e is\nbegin\nend architecture a;\n";
        assert_eq!(ids(src, ""), ["entity_015", "entity_019"]);
        let remove = "rule: {entity_015: {action: remove}, architecture_010: {action: remove}}";
        assert_eq!(ids(src, remove), ["entity_019", "architecture_010"]);
    }

    #[test]
    fn labels_and_conditions() {
        let src = "architecture a of e is\nbegin\n  process begin\n    if rising_edge(c) then null; elsif x then null; end if;\n  end process;\nend architecture a;\n";
        assert_eq!(
            ids(src, ""),
            ["process_012", "process_016", "if_002", "process_018"]
        );
    }

    #[test]
    fn port_default_fix_is_unsafe() {
        let src = "entity e is port (a : in bit := '0'); end entity e;\n";
        let v = crate::rules::check(&Parsed::new(src.as_bytes().to_vec()), &Config::default());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].fix.as_ref().unwrap().safety, FixSafety::Unsafe);
    }
}

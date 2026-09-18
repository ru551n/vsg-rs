//! Naming conventions (prefixes, suffixes, restricted names, reserved words) and comment rules.

use vhdl_syntax::syntax::{NodeKind as N, SyntaxToken};
use vhdl_syntax::tokens::{Keyword as Kw, TokenKind as T, TriviaPiece};

use super::select::{
    child, children, clause_idents, declared, designators, formals, generate_labels, ident,
    idents_of, labels_of, name_idents, text, tokens,
};
use super::{Check, Context, Edit, Fix, FixSafety, Rule, RuleInfo, Violation, violation};
use crate::config::{RuleSettings, Severity};

fn rule(
    id: &'static str,
    groups: &'static [&'static str],
    enabled: bool,
    description: &'static str,
    check: Check,
) -> Rule {
    Rule {
        info: RuleInfo {
            id,
            groups,
            severity: Severity::Error,
            enabled_by_default: enabled,
            description,
        },
        check,
    }
}

type Select = fn(&Context<'_>) -> Vec<SyntaxToken>;

/// A lower-cased list option, or `defaults` if the option is not configured at all.
fn list_option(settings: &RuleSettings, key: &str, defaults: &[&str]) -> Vec<String> {
    let configured = settings.option_list(key);
    let values = if configured.is_empty() && !settings.has_option(key) {
        defaults.to_vec()
    } else {
        configured
    };
    values.iter().map(|v| v.to_ascii_lowercase()).collect()
}

/// Report names that start (`prefix == true`) or end with none of the configured affixes.
fn check_affix(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    rule: &'static str,
    prefix: bool,
    defaults: &[&str],
    select: Select,
) {
    let (key, kind) = if prefix {
        ("prefixes", "Prefix")
    } else {
        ("suffixes", "Suffix")
    };
    let affixes = list_option(settings, key, defaults);
    let shown = {
        let configured = settings.option_list(key);
        if configured.is_empty() && !settings.has_option(key) {
            defaults.join(", ")
        } else {
            configured.join(", ")
        }
    };
    let exceptions = list_option(settings, "exceptions", &[]);
    for t in select(cx) {
        let name = text(&t).to_ascii_lowercase();
        if t.kind() != T::Identifier || exceptions.contains(&name) {
            continue;
        }
        let ok = affixes.iter().any(|a| {
            name.len() > a.len()
                && if prefix {
                    name.starts_with(a.as_str())
                } else {
                    name.ends_with(a.as_str())
                }
        });
        if !ok {
            out.push(violation(
                settings,
                rule,
                &t,
                format!("{kind} {} with one of the following: {shown}", text(&t)),
            ));
        }
    }
}

macro_rules! affix_rules {
    ($($id:literal, $prefix:literal, [$($default:literal),*], $desc:literal, $select:expr;)*) => {
        vec![$(rule($id, &["naming"], false, $desc, |cx, s, out| {
            check_affix(cx, s, out, $id, $prefix, &[$($default),*], $select);
        }),)*]
    };
}

fn ports(cx: &Context<'_>, mode: Kw) -> Vec<SyntaxToken> {
    clause_idents(cx, N::PortClause)
        .into_iter()
        .filter(|(_, m)| m.unwrap_or(Kw::In) == mode)
        .map(|(t, _)| t)
        .collect()
}

fn interface(cx: &Context<'_>, clause: N) -> Vec<SyntaxToken> {
    clause_idents(cx, clause)
        .into_iter()
        .map(|(t, _)| t)
        .collect()
}

fn parameters(cx: &Context<'_>, generate: bool) -> Vec<SyntaxToken> {
    cx.nodes(N::ParameterSpecification)
        .iter()
        .filter(|p| {
            p.parent()
                .is_some_and(|q| q.kind() == N::ForGeneratePreamble)
                == generate
        })
        .filter_map(ident)
        .collect()
}

fn map_formals(cx: &Context<'_>) -> Vec<SyntaxToken> {
    cx.nodes(N::GenericMapAspect)
        .iter()
        .flat_map(formals)
        .collect()
}

fn affix() -> Vec<Rule> {
    affix_rules! {
        "alias_declaration_600", true, ["a_"], "Alias designators have a valid prefix.",
            |cx| idents_of(cx, N::AliasDeclaration);
        "alias_declaration_601", false, ["_a"], "Alias designators have a valid suffix.",
            |cx| idents_of(cx, N::AliasDeclaration);
        "block_600", false, ["_blk"], "Block labels have a valid suffix.",
            |cx| labels_of(cx, N::BlockStatement);
        "block_601", true, ["blk_"], "Block labels have a valid prefix.",
            |cx| labels_of(cx, N::BlockStatement);
        "constant_015", true, ["c_"], "Constant names have a valid prefix.",
            |cx| declared(cx, N::ConstantDeclaration);
        "constant_600", false, ["_c"], "Constant names have a valid suffix.",
            |cx| declared(cx, N::ConstantDeclaration);
        "function_600", true, ["f_"], "Function designators have a valid prefix.",
            |cx| designators(cx, N::FunctionSpecification);
        "function_601", false, ["_f"], "Function designators have a valid suffix.",
            |cx| designators(cx, N::FunctionSpecification);
        "generate_017", true, ["gen_"], "Generate labels have a valid prefix.",
            generate_labels;
        "generate_600", false, ["_gen"], "Generate labels have a valid suffix.",
            generate_labels;
        "generate_601", true, ["gv_"], "Generate parameters have a valid prefix.",
            |cx| parameters(cx, true);
        "generate_602", false, ["_gv"], "Generate parameters have a valid suffix.",
            |cx| parameters(cx, true);
        "generic_020", true, ["g_"], "Generic names have a valid prefix.",
            |cx| interface(cx, N::GenericClause);
        "generic_600", false, ["_g"], "Generic names have a valid suffix.",
            |cx| interface(cx, N::GenericClause);
        "generic_map_600", false, ["_g"], "Generic names in generic maps have a valid suffix.",
            map_formals;
        "generic_map_601", true, ["g_"], "Generic names in generic maps have a valid prefix.",
            map_formals;
        "instantiation_600", false, ["_inst"], "Instance labels have a valid suffix.",
            |cx| labels_of(cx, N::ComponentInstantiationStatement);
        "instantiation_601", true, ["inst_"], "Instance labels have a valid prefix.",
            |cx| labels_of(cx, N::ComponentInstantiationStatement);
        "interface_incomplete_type_declaration_600", true, ["gt_"],
            "Generic type names have a valid prefix.",
            |cx| idents_of(cx, N::InterfaceIncompleteTypeDeclaration);
        "interface_incomplete_type_declaration_601", false, ["_gt"],
            "Generic type names have a valid suffix.",
            |cx| idents_of(cx, N::InterfaceIncompleteTypeDeclaration);
        "loop_statement_600", true, ["loop_"], "Loop labels have a valid prefix.",
            |cx| labels_of(cx, N::LoopStatement);
        "loop_statement_601", false, ["_loop"], "Loop labels have a valid suffix.",
            |cx| labels_of(cx, N::LoopStatement);
        "loop_statement_602", true, ["lv_"], "Loop parameters have a valid prefix.",
            |cx| parameters(cx, false);
        "loop_statement_603", false, ["_lv"], "Loop parameters have a valid suffix.",
            |cx| parameters(cx, false);
        "package_016", false, ["_pkg"], "Package names have a valid suffix.",
            |cx| idents_of(cx, N::PackagePreamble);
        "package_017", true, ["pkg_"], "Package names have a valid prefix.",
            |cx| idents_of(cx, N::PackagePreamble);
        "package_body_600", false, ["_pkg"], "Package body names have a valid suffix.",
            |cx| idents_of(cx, N::PackageBodyPreamble);
        "package_body_601", true, ["pkg_"], "Package body names have a valid prefix.",
            |cx| idents_of(cx, N::PackageBodyPreamble);
        "package_instantiation_600", false, ["_pkg"],
            "Instantiated package names have a valid suffix.",
            |cx| idents_of(cx, N::PackageInstantiationPreamble);
        "package_instantiation_601", true, ["pkg_"],
            "Instantiated package names have a valid prefix.",
            |cx| idents_of(cx, N::PackageInstantiationPreamble);
        "port_011", true, ["i_", "o_", "io_"], "Port names have a valid prefix.",
            |cx| interface(cx, N::PortClause);
        "port_025", false, ["_i", "_o", "_io"], "Port names have a valid suffix.",
            |cx| interface(cx, N::PortClause);
        "port_600", true, ["i_"], "Input port names have a valid prefix.",
            |cx| ports(cx, Kw::In);
        "port_601", true, ["o_"], "Output port names have a valid prefix.",
            |cx| ports(cx, Kw::Out);
        "port_602", true, ["io_"], "Inout port names have a valid prefix.",
            |cx| ports(cx, Kw::Inout);
        "port_603", true, ["b_"], "Buffer port names have a valid prefix.",
            |cx| ports(cx, Kw::Buffer);
        "port_604", true, ["l_"], "Linkage port names have a valid prefix.",
            |cx| ports(cx, Kw::Linkage);
        "port_605", false, ["_i"], "Input port names have a valid suffix.",
            |cx| ports(cx, Kw::In);
        "port_606", false, ["_o"], "Output port names have a valid suffix.",
            |cx| ports(cx, Kw::Out);
        "port_607", false, ["_io"], "Inout port names have a valid suffix.",
            |cx| ports(cx, Kw::Inout);
        "port_608", false, ["_b"], "Buffer port names have a valid suffix.",
            |cx| ports(cx, Kw::Buffer);
        "port_609", false, ["_l"], "Linkage port names have a valid suffix.",
            |cx| ports(cx, Kw::Linkage);
        "process_036", true, ["proc_"], "Process labels have a valid prefix.",
            |cx| labels_of(cx, N::ProcessStatement);
        "process_600", false, ["_proc"], "Process labels have a valid suffix.",
            |cx| labels_of(cx, N::ProcessStatement);
        "signal_008", true, ["s_"], "Signal names have a valid prefix.",
            |cx| declared(cx, N::SignalDeclaration);
        "signal_600", false, ["_s"], "Signal names have a valid suffix.",
            |cx| declared(cx, N::SignalDeclaration);
        "subtype_004", true, ["st_"], "Subtype names have a valid prefix.",
            |cx| idents_of(cx, N::SubtypeDeclaration);
        "subtype_600", false, ["_st"], "Subtype names have a valid suffix.",
            |cx| idents_of(cx, N::SubtypeDeclaration);
        "type_015", true, ["t_"], "Type names have a valid prefix.",
            |cx| idents_of(cx, N::FullTypeDeclaration);
        "type_600", false, ["_t"], "Type names have a valid suffix.",
            |cx| idents_of(cx, N::FullTypeDeclaration);
        "variable_012", true, ["v_"], "Variable names have a valid prefix.",
            |cx| declared(cx, N::VariableDeclaration);
        "variable_600", false, ["_v"], "Variable names have a valid suffix.",
            |cx| declared(cx, N::VariableDeclaration);
    }
}

/// Architecture names must be one of `names` (no restriction while the list is empty).
fn architecture_names(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let names = list_option(settings, "names", &[]);
    if names.is_empty() {
        return;
    }
    for t in idents_of(cx, N::ArchitecturePreamble) {
        if !names.contains(&text(&t).to_ascii_lowercase()) {
            out.push(violation(
                settings,
                "architecture_025",
                &t,
                format!(
                    "Architecture identifier must match a name from this list: {}",
                    settings.option_list("names").join(", ")
                ),
            ));
        }
    }
}

fn restricted_libraries(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let names = list_option(settings, "names", &[]);
    for list in cx.nodes(N::LogicalNameList) {
        for t in tokens(list).filter(|t| t.kind() == T::Identifier) {
            if names.contains(&text(&t).to_ascii_lowercase()) {
                out.push(violation(
                    settings,
                    "library_012",
                    &t,
                    format!("Library name is on list of restricted names: {}", text(&t)),
                ));
            }
        }
    }
}

fn restricted_packages(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let names = list_option(settings, "names", &["std_logic_arith"]);
    for clause in cx.nodes(N::UseClause) {
        let Some(list) = child(clause, N::NameList) else {
            continue;
        };
        for name in children(&list, N::Name) {
            // The package is the second part of `lib.pkg.item` and `lib.pkg`.
            let Some(package) = name_idents(&name).into_iter().nth(1) else {
                continue;
            };
            if names.contains(&text(&package).to_ascii_lowercase()) {
                out.push(violation(
                    settings,
                    "use_clause_001",
                    &package,
                    format!(
                        "Package name is on list of restricted names: {}",
                        text(&package)
                    ),
                ));
            }
        }
    }
}

const RESERVED_1993: &[&str] = &[
    "abs",
    "access",
    "after",
    "alias",
    "all",
    "and",
    "architecture",
    "array",
    "assert",
    "attribute",
    "begin",
    "block",
    "body",
    "buffer",
    "bus",
    "case",
    "component",
    "configuration",
    "constant",
    "disconnect",
    "downto",
    "else",
    "elsif",
    "end",
    "entity",
    "exit",
    "file",
    "for",
    "function",
    "generate",
    "generic",
    "group",
    "guarded",
    "if",
    "impure",
    "in",
    "inertial",
    "inout",
    "is",
    "label",
    "library",
    "linkage",
    "literal",
    "loop",
    "map",
    "mod",
    "nand",
    "new",
    "next",
    "nor",
    "not",
    "null",
    "of",
    "on",
    "open",
    "or",
    "others",
    "out",
    "package",
    "port",
    "postponed",
    "procedure",
    "process",
    "pure",
    "range",
    "record",
    "register",
    "reject",
    "rem",
    "report",
    "return",
    "rol",
    "ror",
    "select",
    "severity",
    "signal",
    "shared",
    "sla",
    "sll",
    "sra",
    "srl",
    "subtype",
    "then",
    "to",
    "transport",
    "type",
    "unaffected",
    "units",
    "until",
    "use",
    "variable",
    "wait",
    "when",
    "while",
    "with",
    "xnor",
    "xor",
];
const RESERVED_2008: &[&str] = &[
    "assume",
    "assume_guarantee",
    "context",
    "cover",
    "default",
    "fairness",
    "force",
    "parameter",
    "property",
    "protected",
    "release",
    "restrict",
    "restrict_guarantee",
    "sequence",
    "strong",
    "vmode",
    "vprop",
    "vunit",
];
const RESERVED_2019: &[&str] = &["private", "view"];
/// VHDL-AMS (IEEE 1076.1), included by `standard: all` as in VSG.
const RESERVED_AMS: &[&str] = &[
    "across",
    "break",
    "limit",
    "nature",
    "noise",
    "procedural",
    "quantity",
    "reference",
    "spectrum",
    "subnature",
    "terminal",
    "through",
    "tolerance",
];

fn reserved_words(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let sets: &[&[&str]] = match settings.option_str("standard").unwrap_or("all") {
        "1993" | "93" => &[RESERVED_1993],
        "2008" | "08" => &[RESERVED_1993, RESERVED_2008],
        _ => &[RESERVED_1993, RESERVED_2008, RESERVED_2019, RESERVED_AMS],
    };
    for t in cx.parsed.tokens() {
        let name = text(t).to_ascii_lowercase();
        // As in VSG, only where the identifier is declared.
        if t.kind() == T::Identifier
            && super::select::is_declaration_position(t)
            && sets.iter().any(|s| s.contains(&name.as_str()))
        {
            out.push(violation(
                settings,
                "reserved_001",
                t,
                format!("Invalid use of reserved word {}", text(t)),
            ));
        }
    }
}

/// A line comment in the source.
struct CommentAt {
    start: usize,
    /// Without trailing whitespace.
    text: String,
    /// On the same line as the previous token.
    trailing: bool,
    /// Index of the token whose leading trivia holds the comment.
    token: usize,
}

fn comments(cx: &Context<'_>) -> Vec<CommentAt> {
    let mut out = Vec::new();
    for (i, t) in cx.parsed.tokens().iter().enumerate() {
        let trivia = t.leading_trivia();
        let len: usize = trivia.into_iter().map(TriviaPiece::byte_len).sum();
        let mut offset = t.text_offset() - len;
        let mut newline = i == 0;
        for piece in trivia {
            if let TriviaPiece::LineComment(c) = piece {
                out.push(CommentAt {
                    start: offset,
                    text: String::from_utf8_lossy(c.as_bytes()).trim_end().to_owned(),
                    trailing: !newline,
                    token: i,
                });
            }
            newline |= piece.is_newline();
            offset += piece.byte_len();
        }
    }
    out
}

fn comment_violation(
    settings: &RuleSettings,
    rule: &'static str,
    c: &CommentAt,
    message: String,
) -> Violation {
    Violation {
        rule,
        severity: settings.severity,
        start: c.start,
        end: c.start + c.text.len(),
        message,
        fix: None,
    }
}

fn comment_keywords(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let configured = settings.option_list("keywords");
    let keywords = if configured.is_empty() && !settings.has_option("keywords") {
        vec!["TODO", "FIXME"]
    } else {
        configured
    };
    for c in comments(cx) {
        if let Some(k) = keywords.iter().find(|k| c.text.contains(**k)) {
            out.push(comment_violation(
                settings,
                "comment_012",
                &c,
                format!("Comment keyword {k} detected."),
            ));
        }
    }
}

/// Move a trailing comment onto its own line above the line it ends.
fn inline_comments(cx: &Context<'_>, settings: &RuleSettings, out: &mut Vec<Violation>) {
    let source = cx.parsed.source();
    let newline = if cx.parsed.uses_crlf() { "\r\n" } else { "\n" };
    for c in comments(cx)
        .into_iter()
        .filter(|c| c.trailing && c.token > 0)
    {
        let line_start = source[..c.start]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);
        let indent_end = source[line_start..c.start]
            .iter()
            .position(|&b| !matches!(b, b' ' | b'\t'))
            .map_or(c.start, |p| line_start + p);
        let indent = String::from_utf8_lossy(&source[line_start..indent_end]);
        let code_end = source[..c.start]
            .iter()
            .rposition(|&b| !matches!(b, b' ' | b'\t'))
            .map_or(c.start, |p| p + 1);
        let mut v = comment_violation(
            settings,
            "comment_011",
            &c,
            "Move inline comment to previous line.".into(),
        );
        v.fix = Some(Fix {
            safety: FixSafety::Safe,
            edits: vec![
                Edit {
                    start: line_start,
                    end: line_start,
                    text: format!("{indent}{}{newline}", c.text),
                    rank: 0,
                },
                Edit {
                    start: code_end,
                    end: c.start + c.text.len(),
                    text: String::new(),
                    rank: 0,
                },
            ],
        });
        out.push(v);
    }
}

/// Runs of at least `min_height` full-line comments, one per line, starting with a rule line.
fn comment_blocks(cx: &Context<'_>, settings: &RuleSettings) -> Vec<Vec<CommentAt>> {
    let min = settings.option_usize("min_height").unwrap_or(3).max(2);
    let source = cx.parsed.source();
    let mut blocks: Vec<Vec<CommentAt>> = Vec::new();
    let mut current: Vec<CommentAt> = Vec::new();
    for c in comments(cx).into_iter().filter(|c| !c.trailing) {
        let adjacent = current.last().is_some_and(|p| {
            let gap = &source[p.start + p.text.len()..c.start];
            p.token == c.token && gap.iter().filter(|&&b| b == b'\n').count() == 1
        });
        if !adjacent && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    blocks.push(current);
    blocks.retain(|b| b.len() >= min && is_rule_line(&b[0].text));
    blocks
}

/// `--` followed by at least two punctuation characters (a header such as `--+-----`).
fn is_rule_line(text: &str) -> bool {
    let body = text.strip_prefix("--").unwrap_or("");
    body.chars().count() >= 2
        && body
            .chars()
            .take(2)
            .all(|ch| !ch.is_alphanumeric() && !ch.is_whitespace())
}

/// Check the header (`footer == false`) or footer line of a block comment.
fn check_edge(settings: &RuleSettings, line: &str, footer: bool) -> Option<String> {
    let edge = if footer { "footer" } else { "header" };
    let opt = |k: &str| {
        settings
            .option_str(&format!("{edge}_{k}"))
            .map(str::to_owned)
    };
    let left = opt("left").unwrap_or_default();
    let left_repeat = opt("left_repeat").unwrap_or_else(|| "-".into());
    let right_repeat = opt("right_repeat").unwrap_or_else(|| left_repeat.clone());
    let title = opt("string").unwrap_or_default();
    let max = settings
        .option_usize(&format!("max_{edge}_column"))
        .unwrap_or(120);
    let Some(rest) = line
        .strip_prefix("--")
        .and_then(|r| r.strip_prefix(left.as_str()))
    else {
        return Some(format!("must start with `--{left}`"));
    };
    let (before, after) = if title.is_empty() {
        (rest, "")
    } else if let Some(p) = rest.find(title.as_str()) {
        (&rest[..p], &rest[p + title.len()..])
    } else {
        return Some(format!("must contain `{title}`"));
    };
    if !before.chars().all(|c| left_repeat.contains(c) || c == ' ') {
        return Some(format!("must be filled with `{left_repeat}`"));
    }
    if !after.chars().all(|c| right_repeat.contains(c) || c == ' ') {
        return Some(format!("must end with `{right_repeat}`"));
    }
    (line.chars().count() > max).then(|| format!("must not extend past column {max}"))
}

/// The header (`footer == false`) or footer line the configuration asks for.
fn expected_edge(settings: &RuleSettings, footer: bool) -> String {
    let edge = if footer { "footer" } else { "header" };
    let opt = |k: &str| {
        settings
            .option_str(&format!("{edge}_{k}"))
            .map(str::to_owned)
    };
    let left = opt("left").unwrap_or_default();
    let left_repeat = opt("left_repeat").unwrap_or_else(|| "-".into());
    let right_repeat = opt("right_repeat").unwrap_or_else(|| left_repeat.clone());
    let title = opt("string").unwrap_or_default();
    let max = settings
        .option_usize(&format!("max_{edge}_column"))
        .unwrap_or(120);
    let fill = max.saturating_sub(2 + left.chars().count() + title.chars().count());
    let before = match opt("alignment").as_deref() {
        Some("left") => 0,
        Some("right") => fill,
        _ => fill / 2,
    };
    let repeat = |s: &str, n: usize| s.chars().cycle().take(n).collect::<String>();
    format!(
        "--{left}{}{title}{}",
        repeat(&left_repeat, if title.is_empty() { fill } else { before }),
        if title.is_empty() {
            String::new()
        } else {
            repeat(&right_repeat, fill - before)
        }
    )
}

#[derive(Clone, Copy)]
enum BlockPart {
    Header,
    Body,
    Footer,
}

fn block_comment(
    cx: &Context<'_>,
    settings: &RuleSettings,
    out: &mut Vec<Violation>,
    part: BlockPart,
) {
    let id = match part {
        BlockPart::Header => "block_comment_001",
        BlockPart::Body => "block_comment_002",
        BlockPart::Footer => "block_comment_003",
    };
    for block in comment_blocks(cx, settings) {
        let last = &block[block.len() - 1];
        let problems: Vec<(&CommentAt, String)> = match part {
            BlockPart::Header => check_edge(settings, &block[0].text, false)
                .map(|_| {
                    let expected = expected_edge(settings, false);
                    (
                        &block[0],
                        format!("Change block comment header to : {expected}"),
                    )
                })
                .into_iter()
                .collect(),
            BlockPart::Footer => check_edge(settings, &last.text, true)
                .map(|_| {
                    let expected = expected_edge(settings, true);
                    (last, format!("Change block comment footer to : {expected}"))
                })
                .into_iter()
                .collect(),
            BlockPart::Body => {
                let Some(left) = settings.option_str("comment_left") else {
                    continue;
                };
                let prefix = format!("--{left}");
                block[1..block.len() - 1]
                    .iter()
                    .filter(|c| !c.text.starts_with(&prefix))
                    .map(|c| (c, format!("Comment must start with {prefix}")))
                    .collect()
            }
        };
        for (c, message) in problems {
            out.push(comment_violation(settings, id, c, message));
        }
    }
}

pub(super) fn rules() -> Vec<Rule> {
    let mut all = affix();
    all.extend([
        rule(
            "architecture_025",
            &["naming"],
            false,
            "Architecture names are one of the configured names.",
            architecture_names,
        ),
        rule(
            "library_012",
            &["naming"],
            false,
            "Restricted libraries are not used.",
            restricted_libraries,
        ),
        rule(
            "use_clause_001",
            &["naming"],
            false,
            "Restricted packages are not used.",
            restricted_packages,
        ),
        rule(
            "reserved_001",
            &["naming"],
            true,
            "Words reserved in any VHDL standard are not used as identifiers.",
            reserved_words,
        ),
        Rule {
            info: RuleInfo {
                severity: Severity::Warning,
                ..rule(
                    "comment_012",
                    &[],
                    false,
                    "Comments do not contain the configured keywords.",
                    comment_keywords,
                )
                .info
            },
            check: comment_keywords,
        },
        rule(
            "comment_011",
            &["structure"],
            false,
            "Comments are not placed after code.",
            inline_comments,
        ),
        rule(
            "block_comment_001",
            &[],
            false,
            "Block comment headers follow the configured pattern.",
            |cx, s, out| block_comment(cx, s, out, BlockPart::Header),
        ),
        rule(
            "block_comment_002",
            &[],
            false,
            "Block comment lines start with the configured text.",
            |cx, s, out| block_comment(cx, s, out, BlockPart::Body),
        ),
        rule(
            "block_comment_003",
            &[],
            false,
            "Block comment footers follow the configured pattern.",
            |cx, s, out| block_comment(cx, s, out, BlockPart::Footer),
        ),
    ]);
    all
}

#[cfg(test)]
mod tests {
    use crate::Parsed;
    use crate::config::Config;

    fn found(src: &str, yaml: &str, prefix: &str) -> Vec<(&'static str, String)> {
        let config = Config::parse(yaml).unwrap();
        crate::rules::check(&Parsed::new(src.as_bytes().to_vec()), &config)
            .into_iter()
            .filter(|v| v.rule.starts_with(prefix))
            .map(|v| (v.rule, src[v.start..v.end].to_owned()))
            .collect()
    }

    #[test]
    fn prefixes_and_restrictions() {
        let src = "library bad;\nuse ieee.std_logic_arith.all;\nentity e is\n  port (i_a : in bit; b : out bit; view : in bit);\nend;\n";
        let yaml = "rule:\n  port_600:\n    disable: false\n  port_601:\n    disable: false\n  library_012:\n    disable: false\n    names: [bad]\n  use_clause_001:\n    disable: false\n  reserved_001:\n    disable: false\n";
        let got: Vec<(&str, String)> = ["library_012", "use_clause_001", "port_6", "reserved"]
            .iter()
            .flat_map(|p| found(src, yaml, p))
            .collect();
        let expect = [
            ("library_012", "bad"),
            ("use_clause_001", "std_logic_arith"),
            ("port_601", "b"),
            ("port_600", "view"),
            ("reserved_001", "view"),
        ];
        assert_eq!(
            got,
            expect.map(|(r, t)| (r, t.to_owned())),
            "port_600 must accept `i_a`"
        );
    }

    #[test]
    fn comment_rules() {
        let src = "-- TODO: x\nentity e is -- trailing\nend entity e;\n";
        let yaml =
            "rule:\n  comment_012:\n    disable: false\n  comment_011:\n    disable: false\n";
        assert_eq!(
            found(src, yaml, "comment_012"),
            [("comment_012", "-- TODO: x".into())]
        );
        assert_eq!(
            found(src, yaml, "comment_011"),
            [("comment_011", "-- trailing".into())]
        );
        let config = Config::parse(yaml).unwrap();
        let out = crate::fix::fix(&Parsed::new(src.as_bytes().to_vec()), &config).unwrap();
        assert_eq!(
            String::from_utf8(out.output).unwrap(),
            "-- TODO: x\n-- trailing\nentity e is\nend entity e;\n"
        );
    }

    #[test]
    fn block_comments() {
        let src = "--------\n-- Title\n-- body\n--======\nentity e is\nend entity e;\n";
        let yaml = "rule:\n  block_comment_001:\n    disable: false\n  block_comment_002:\n    disable: false\n    comment_left: '|'\n  block_comment_003:\n    disable: false\n";
        let rules: Vec<&str> = found(src, yaml, "block").iter().map(|(r, _)| *r).collect();
        assert_eq!(
            rules,
            [
                "block_comment_002",
                "block_comment_002",
                "block_comment_003"
            ]
        );
    }
}

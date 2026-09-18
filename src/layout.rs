//! What formatting changes, token by token, and which VSG layout rule reports it.
//!
//! The formatted snapshot has the same tokens as the source, so the two token streams are
//! compared pairwise. Each difference in the whitespace before a token (indentation, spaces,
//! line breaks, blank lines, trailing whitespace, comment column) or in a keyword's case becomes
//! a [`LayoutChange`]. Keyword case maps to its VSG rule directly. The other kinds map to VSG
//! rules through a table learned by comparing with VSG's reports on real code
//! (`scripts/learn_layout_rules.py`); unknown changes are reported under `format`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use vhdl_syntax::syntax::{NodeKind, SyntaxNode, SyntaxToken};
use vhdl_syntax::tokens::TokenKind as T;

use crate::Parsed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// The indentation of a line.
    Indent,
    /// Spaces between two tokens on one line.
    Spacing,
    /// A token moves to a new line.
    LineBreak,
    /// A line break before a token is removed.
    Join,
    /// The number of blank lines before a line.
    BlankLines,
    /// Whitespace at the end of a line.
    Trailing,
    /// The case of a keyword.
    KeywordCase,
    /// The column of a trailing comment.
    CommentColumn,
}

/// One layout difference between a snapshot and its formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutChange {
    /// 1-based line in the source.
    pub line: usize,
    pub kind: ChangeKind,
    /// Context of the change for the rule table, such as `Indent:ProcessStatementPart:signal`.
    pub key: String,
    /// The VSG rule that reports the change, if known without the table.
    pub rule: Option<&'static str>,
    /// Expected indentation or spaces (columns), or blank lines.
    pub expected: usize,
    /// The token the change is about, as formatted.
    pub token: String,
    /// The token as written in the source.
    pub original: String,
}

/// The whitespace and comments between two tokens: `src` between the previous token and
/// token `i`.
fn gap<'a>(src: &'a [u8], tokens: &[SyntaxToken], i: usize) -> &'a [u8] {
    let start = if i == 0 {
        0
    } else {
        tokens[i - 1].text_range().end
    };
    &src[start..tokens[i].text_offset()]
}

fn newlines(gap: &[u8]) -> usize {
    gap.iter().filter(|&&b| b == b'\n').count()
}

fn has_comment(gap: &[u8]) -> bool {
    gap.windows(2).any(|w| w == b"--" || w == b"/*")
}

/// Width of the whitespace after the last line break (or of the whole gap).
fn tail(gap: &[u8]) -> usize {
    let start = gap.iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
    gap[start..]
        .iter()
        .filter(|&&b| matches!(b, b' ' | b'\t'))
        .count()
}

/// Whether a line that ends in the gap ends with spaces or tabs.
fn trailing_whitespace(gap: &[u8]) -> bool {
    gap.split(|&b| b == b'\n').rev().skip(1).any(|line| {
        line.strip_suffix(b"\r")
            .unwrap_or(line)
            .last()
            .is_some_and(|b| matches!(b, b' ' | b'\t'))
    })
}

/// Spaces before a comment on the same line as the previous token.
fn comment_offset(gap: &[u8]) -> Option<usize> {
    let first = gap.windows(2).position(|w| w == b"--" || w == b"/*")?;
    let before = &gap[..first];
    (!before.contains(&b'\n')).then_some(before.len())
}

fn kind_name(t: &SyntaxToken) -> String {
    match t.kind() {
        T::Keyword(k) => format!("{k:?}").to_ascii_lowercase(),
        other => format!("{other:?}"),
    }
}

fn construct(t: &SyntaxToken) -> String {
    format!("{:?}", t.parent().kind())
}

/// The `case::keyword` rule for a keyword: the one for the innermost construct that has one.
fn keyword_rule(t: &SyntaxToken) -> Option<&'static str> {
    let word = String::from_utf8_lossy(t.text().as_bytes()).to_ascii_lowercase();
    let candidates: Vec<&'static crate::rules::RuleInfo> = crate::keywords::RULES
        .iter()
        .filter(|(_, words)| words.contains(&word.as_str()))
        .map(|(info, _)| info)
        .collect();
    let mut node: Option<SyntaxNode> = Some(t.parent());
    while let Some(n) = node {
        let kind = crate::format::construct_kind(&n);
        if let Some(info) = candidates
            .iter()
            .find(|info| crate::keywords::constructs(info.id).contains(&kind))
        {
            return Some(info.id);
        }
        node = n.parent();
    }
    candidates
        .iter()
        .find(|info| crate::keywords::constructs(info.id).is_empty())
        .map(|info| info.id)
}

/// The layout differences between `before` and its formatting `after` (same tokens).
pub fn layout_changes(before: &Parsed, after: &Parsed) -> Vec<LayoutChange> {
    let (bt, at) = (before.tokens(), after.tokens());
    if bt.len() != at.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, (b, a)) in bt.iter().zip(at).enumerate() {
        let line = before.line_col(b.text_offset()).0;
        let token = String::from_utf8_lossy(a.text().as_bytes()).into_owned();
        let original = String::from_utf8_lossy(b.text().as_bytes()).into_owned();
        let mut push = |kind, key: String, rule, expected, line| {
            out.push(LayoutChange {
                line,
                kind,
                key,
                rule,
                expected,
                token: token.clone(),
                original: original.clone(),
            });
        };
        if b.text().as_bytes() != a.text().as_bytes() && matches!(b.kind(), T::Keyword(_)) {
            push(
                ChangeKind::KeywordCase,
                format!("KeywordCase:{}", kind_name(b)),
                keyword_rule(b),
                0,
                line,
            );
        }
        let (gb, ga) = (gap(before.source(), bt, i), gap(after.source(), at, i));
        if gb == ga {
            continue;
        }
        let (nb, na) = (newlines(gb), newlines(ga));
        let prev = if i == 0 {
            "Start".to_owned()
        } else {
            kind_name(&bt[i - 1])
        };
        let here = format!("{}:{}", construct(b), kind_name(b));
        if trailing_whitespace(gb) && !trailing_whitespace(ga) {
            let first_line = line.saturating_sub(nb).max(1);
            push(ChangeKind::Trailing, "Trailing".into(), None, 0, first_line);
        }
        if has_comment(gb) || has_comment(ga) {
            if let (Some(ob), Some(oa)) = (comment_offset(gb), comment_offset(ga))
                && ob != oa
                && i > 0
            {
                push(
                    ChangeKind::CommentColumn,
                    format!("CommentColumn:{}", construct(&bt[i - 1])),
                    None,
                    oa,
                    line.saturating_sub(nb),
                );
            }
            // Own-line comments keep their layout; only the token's indentation counts.
            if nb > 0 && na > 0 && tail(gb) != tail(ga) {
                push(
                    ChangeKind::Indent,
                    format!("Indent:{here}"),
                    None,
                    tail(ga),
                    line,
                );
            }
            continue;
        }
        match (nb, na) {
            (0, 0) => push(
                ChangeKind::Spacing,
                format!("Spacing:{}:{prev}>{}", construct(b), kind_name(b)),
                None,
                tail(ga),
                line,
            ),
            (0, _) => push(
                ChangeKind::LineBreak,
                format!("LineBreak:{here}"),
                None,
                tail(ga),
                line,
            ),
            (_, 0) => push(ChangeKind::Join, format!("Join:{here}"), None, 0, line),
            (x, y) => {
                if x != y {
                    push(
                        ChangeKind::BlankLines,
                        format!("BlankLines:{here}"),
                        None,
                        y - 1,
                        line,
                    );
                }
                if tail(gb) != tail(ga) {
                    push(
                        ChangeKind::Indent,
                        format!("Indent:{here}"),
                        None,
                        tail(ga),
                        line,
                    );
                }
            }
        }
    }
    out
}

/// The learned `key → VSG rule` table.
fn table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE
        .get_or_init(|| serde_json::from_str(include_str!("layout_rules.json")).unwrap_or_default())
}

/// Spaces a VSG `number_of_spaces` option asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spaces {
    Exact(usize),
    /// At least this many; wider source spacing is kept (`>=1`).
    AtLeast(usize),
}

/// A configured spacing between two tokens (`None`: any token) inside some constructs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpacingRule {
    pub kinds: &'static [NodeKind],
    pub prev: Option<&'static str>,
    pub next: Option<&'static str>,
    pub spaces: Spaces,
}

type PairTable = BTreeMap<String, Vec<[Option<String>; 2]>>;

/// The token pairs each VSG spacing rule checks, from its documentation
/// (`scripts/gen_spacing_rules.py`).
pub(crate) fn spacing_pairs() -> &'static PairTable {
    static TABLE: OnceLock<PairTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("spacing_rules.json")).unwrap_or_default()
    })
}

/// The constructs any spacing rule applies to.
fn spacing_constructs() -> &'static HashSet<NodeKind> {
    static KINDS: OnceLock<HashSet<NodeKind>> = OnceLock::new();
    KINDS.get_or_init(|| {
        spacing_pairs()
            .keys()
            .flat_map(|rule| crate::keywords::constructs(rule).iter().copied())
            .collect()
    })
}

/// Whether `t` is what a spacing matcher names: a keyword, `identifier` or a symbol.
fn token_is(name: &str, t: &SyntaxToken) -> bool {
    match t.kind() {
        T::Keyword(_) => kind_name(t) == name,
        T::Identifier => name == "identifier",
        _ => t.text().as_bytes() == name.as_bytes(),
    }
}

/// Whether the token before a spacing is what `name` names. A keyword after `end` only
/// matches `end <keyword>`, as closing and opening rules differ.
fn prev_is(name: &str, t: &SyntaxToken, parsed: &Parsed) -> bool {
    let after_end = matches!(t.kind(), T::Keyword(_))
        && parsed
            .prev_token(t)
            .is_some_and(|p| p.kind() == T::Keyword(vhdl_syntax::tokens::Keyword::End));
    match name.strip_prefix("end ") {
        Some(keyword) => after_end && token_is(keyword, t),
        None => !after_end && token_is(name, t),
    }
}

/// The spaces the first matching rule asks for between `prev` and `t`.
pub(crate) fn spacing_for(
    parsed: &Parsed,
    rules: &[SpacingRule],
    prev: &SyntaxToken,
    t: &SyntaxToken,
) -> Option<Spaces> {
    rules
        .iter()
        .find(|r| spacing_applies(parsed, r, prev, t))
        .map(|r| r.spaces)
}

fn spacing_applies(
    parsed: &Parsed,
    rule: &SpacingRule,
    prev: &SyntaxToken,
    t: &SyntaxToken,
) -> bool {
    if rule.prev.is_some_and(|m| !prev_is(m, prev, parsed))
        || rule.next.is_some_and(|m| !token_is(m, t))
    {
        return false;
    }
    if rule.kinds.is_empty() {
        return true;
    }
    let sides = [(rule.prev, prev), (rule.next, t)];
    // The construct is the one of the most specific named token: keyword, symbol, identifier.
    let Some(anchor) = sides
        .iter()
        .filter(|(m, _)| m.is_some())
        .min_by_key(|(m, tok)| match tok.kind() {
            T::Keyword(_) => 0,
            _ if *m == Some("identifier") => 2,
            _ => 1,
        })
        .map(|(_, tok)| *tok)
    else {
        return false;
    };
    // ponytail: the owner is looked for three levels up only, so that names deep inside
    // expressions do not count; a per-rule path would be exact.
    let mut node = Some(anchor.parent());
    for _ in 0..3 {
        let Some(n) = node else { break };
        let kind = crate::format::construct_kind(&n);
        if spacing_constructs().contains(&kind) {
            return rule.kinds.contains(&kind);
        }
        node = n.parent();
    }
    false
}

/// The VSG rule that reports a change: known directly, from the learned table, or `format`.
pub fn rule_for(change: &LayoutChange) -> &str {
    if let Some(rule) = change.rule {
        return rule;
    }
    if let Some(rule) = table().get(&change.key) {
        return rule;
    }
    match change.kind {
        ChangeKind::Trailing => "whitespace_001",
        _ => "format",
    }
}

/// A VSG-style solution text for a change.
pub fn message(change: &LayoutChange, indent_size: usize) -> String {
    let token = &change.token;
    match change.kind {
        ChangeKind::Indent => format!("Indent level {}", change.expected / indent_size.max(1)),
        ChangeKind::Spacing if change.expected == 0 => {
            format!("Remove the space before {token}")
        }
        ChangeKind::Spacing => format!(
            "Change the number of spaces before {token} to {}",
            change.expected
        ),
        ChangeKind::LineBreak => format!("Move {token} to the next line"),
        ChangeKind::Join => format!("Move {token} to the previous line"),
        ChangeKind::BlankLines if change.expected == 0 => "Remove blank lines above".into(),
        ChangeKind::BlankLines => "Add blank line above".into(),
        ChangeKind::Trailing => "Remove trailing whitespace".into(),
        ChangeKind::KeywordCase => format!("Change \"{}\" to \"{token}\"", change.original),
        ChangeKind::CommentColumn => "Align the comment".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_changes() {
        let src = "ENTITY e is\nport (a:in bit);  \n\n\nend;\n";
        let before = Parsed::new(src.as_bytes().to_vec());
        let cfg = crate::FormatConfig::default();
        let after = Parsed::new(crate::format_parsed(&before, &cfg).unwrap());
        let changes = layout_changes(&before, &after);
        let kinds: Vec<ChangeKind> = changes.iter().map(|c| c.kind).collect();
        for kind in [
            ChangeKind::KeywordCase,
            ChangeKind::Indent,
            ChangeKind::LineBreak,
            ChangeKind::Spacing,
            ChangeKind::Trailing,
            ChangeKind::BlankLines,
        ] {
            assert!(kinds.contains(&kind), "{kind:?} missing in {changes:#?}");
        }
        let case = changes
            .iter()
            .find(|c| c.kind == ChangeKind::KeywordCase)
            .unwrap();
        assert_eq!((case.line, case.rule), (1, Some("entity_004")));
        let trailing = changes
            .iter()
            .find(|c| c.kind == ChangeKind::Trailing)
            .unwrap();
        assert_eq!(trailing.line, 2);
    }
}

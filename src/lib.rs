//! vsg-rs: a VHDL formatter and style checker with VSG-compatible rules and configuration.
//!
//! The pipeline for one source snapshot is: parse once ([`Parsed::new`]), then format and/or
//! lint the same tree. Formatting output is verified by re-parsing it and comparing the token
//! stream and comments against the original before it is returned.

pub mod align;
pub mod analysis;
pub mod blank;
pub mod config;
mod doc;
mod fix;
mod format;
pub mod indent;
mod keywords;
pub mod layout;
mod reflow;
pub mod rules;
mod verify;
pub mod vsg_defaults;

#[cfg(test)]
mod fuzz;

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use vhdl_syntax::parser::parse_with_standard;
use vhdl_syntax::standard::VHDLStandard;
use vhdl_syntax::syntax::{AstNode, SyntaxElement, SyntaxNode, SyntaxToken, TokenKind};
use vhdl_syntax::tokens::TriviaPiece;

pub use config::{Config, FormatConfig};
pub use doc::display_width;
pub use fix::{FixOptions, FixOutcome, fix, fix_with};

/// One immutable source snapshot and its (single) parse.
pub struct Parsed {
    source: Vec<u8>,
    root: SyntaxNode,
    errors: Vec<Diagnostic>,
    standard: VHDLStandard,
    /// All tokens in source order (the tree's own sibling navigation is not constant-time).
    tokens: Vec<SyntaxToken>,
    /// VHDL-2019 tool directives (`` `if ``, `` `warning `` …), which the parser does not accept.
    /// The tree is built from a copy of the source in which each directive line is turned into
    /// a comment of the same length (`` `i `` → `--`), so offsets are unchanged. Formatted output
    /// gets the directives back (see [`Parsed::restore_directives`]).
    directives: Vec<Directive>,
    utf8: bool,
    /// Byte offset of the start of each line, computed on first use.
    line_starts: std::sync::OnceLock<Vec<usize>>,
}

#[derive(Debug, Clone)]
struct Directive {
    /// The first two bytes of the directive, replaced by `--` in the parsed text.
    prefix: [u8; 2],
    /// The directive as a comment (trailing whitespace excluded).
    masked: Vec<u8>,
}

/// A located message about the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub offset: usize,
    pub message: String,
}

impl Parsed {
    pub fn new(source: Vec<u8>) -> Parsed {
        let standard = VHDLStandard::VHDL2008;
        let parse = |text: &[u8]| {
            let (file, errors) = parse_with_standard(standard, text);
            let errors: Vec<Diagnostic> = errors
                .iter()
                .map(|e| Diagnostic {
                    offset: e.span().start,
                    message: format!("{:?}", e.err()),
                })
                .collect();
            let root = file.raw();
            let mut tokens = Vec::new();
            collect_tokens(&root, &mut tokens);
            (root, errors, tokens)
        };
        let (mut root, mut errors, mut tokens) = parse(&source);
        let mut directives = Vec::new();
        if tokens.iter().any(|t| t.kind() == TokenKind::ToolDirective) {
            let mut masked = source.clone();
            for t in tokens
                .iter()
                .filter(|t| t.kind() == TokenKind::ToolDirective)
            {
                let r = t.text_range();
                // Only directives that end their line can become comments.
                let rest = &source[r.end..];
                let line_end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
                if r.len() < 2 || !rest[..line_end].iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                masked[r.start..r.start + 2].copy_from_slice(b"--");
                directives.push(Directive {
                    prefix: [source[r.start], source[r.start + 1]],
                    masked: masked[r.clone()].to_vec(),
                });
            }
            if !directives.is_empty() {
                (root, errors, tokens) = parse(&masked);
            }
        }
        Parsed {
            utf8: std::str::from_utf8(&source).is_ok(),
            source,
            root,
            errors,
            standard,
            tokens,
            directives,
            line_starts: std::sync::OnceLock::new(),
        }
    }

    /// Put the tool directives of this snapshot back into `output`, a formatting of it (in which
    /// they are comments, in the same order).
    fn restore_directives(&self, mut output: Vec<u8>) -> Vec<u8> {
        if self.directives.is_empty() {
            return output;
        }
        let comments: Vec<(usize, usize)> = {
            let printed = Parsed::new(output.clone());
            let mut found = Vec::new();
            for t in &printed.tokens {
                let trivia = t.leading_trivia();
                let len: usize = trivia.into_iter().map(TriviaPiece::byte_len).sum();
                let mut offset = t.text_offset() - len;
                for piece in trivia {
                    if let TriviaPiece::LineComment(c) = piece {
                        found.push((offset, c.as_bytes().len()));
                    }
                    offset += piece.byte_len();
                }
            }
            found
        };
        // Directives are printed at the start of their line.
        let mut restored: Vec<usize> = Vec::new();
        let mut pending = self.directives.iter().peekable();
        for (offset, len) in comments {
            let Some(d) = pending.peek() else { break };
            let text = &output[offset..offset + len];
            let trimmed = text.trim_ascii_end();
            if trimmed == d.masked.as_slice() {
                output[offset..offset + 2].copy_from_slice(&d.prefix);
                restored.push(offset);
                pending.next();
            } else if trimmed.len() == d.masked.len()
                && trimmed[..2] == d.prefix
                && trimmed[2..] == d.masked[2..]
            {
                // Printed verbatim (formatting was off there).
                pending.next();
            }
        }
        let mut result = Vec::with_capacity(output.len());
        let mut pos = 0;
        for offset in restored {
            let line_start = output[..offset]
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(0, |p| p + 1);
            if output[line_start..offset]
                .iter()
                .all(|b| matches!(b, b' ' | b'\t'))
            {
                result.extend_from_slice(&output[pos..line_start]);
                pos = offset;
            }
        }
        result.extend_from_slice(&output[pos..]);
        result
    }

    pub fn source(&self) -> &[u8] {
        &self.source
    }

    pub fn root(&self) -> &SyntaxNode {
        &self.root
    }

    pub fn syntax_errors(&self) -> &[Diagnostic] {
        &self.errors
    }

    pub fn is_utf8(&self) -> bool {
        self.utf8
    }

    /// Whether the source uses CRLF line endings (decided by its first line break).
    pub fn uses_crlf(&self) -> bool {
        self.source
            .iter()
            .position(|&b| b == b'\n')
            .is_some_and(|i| i > 0 && self.source[i - 1] == b'\r')
    }

    /// 1-based line and column (in characters) of a byte offset.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.source.len());
        let starts = self.line_starts.get_or_init(|| {
            std::iter::once(0)
                .chain(
                    self.source
                        .iter()
                        .enumerate()
                        .filter(|&(_, &b)| b == b'\n')
                        .map(|(i, _)| i + 1),
                )
                .collect()
        });
        let line = starts.partition_point(|&s| s <= offset);
        let line_start = starts[line - 1];
        (
            line,
            display_width(&self.source[line_start..offset], self.utf8, 0) + 1,
        )
    }

    pub(crate) fn tokens(&self) -> &[SyntaxToken] {
        &self.tokens
    }

    /// Position of `t` in [`Parsed::tokens`]. Token text offsets are strictly increasing.
    pub(crate) fn token_index(&self, t: &SyntaxToken) -> Option<usize> {
        let offset = t.text_offset();
        let i = self.tokens.partition_point(|x| x.text_offset() < offset);
        (self.tokens.get(i)?.text_offset() == offset).then_some(i)
    }

    pub(crate) fn prev_token(&self, t: &SyntaxToken) -> Option<&SyntaxToken> {
        let i = self.token_index(t)?.checked_sub(1)?;
        self.tokens.get(i)
    }

    pub(crate) fn next_token(&self, t: &SyntaxToken) -> Option<&SyntaxToken> {
        self.tokens.get(self.token_index(t)? + 1)
    }

    /// True when the file contains nothing but whitespace and comments.
    pub fn is_blank(&self) -> bool {
        self.tokens.iter().all(|t| t.kind() == TokenKind::Eof)
    }
}

/// Append the tokens of `node` in source order (linear in the size of the node).
pub(crate) fn collect_tokens(node: &SyntaxNode, out: &mut Vec<SyntaxToken>) {
    for child in node.children_with_tokens() {
        match child {
            SyntaxElement::Token(t) => out.push(t),
            SyntaxElement::Node(n) => collect_tokens(&n, out),
        }
    }
}

#[derive(Debug)]
pub enum FormatError {
    /// The source does not parse; it is left untouched.
    Syntax(Vec<Diagnostic>),
    /// The source uses a construct the formatter does not handle safely yet.
    Unsupported(Diagnostic),
    /// The formatter produced output that is not equivalent to the input (a formatter bug).
    /// The source is left untouched.
    Internal(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Syntax(d) => write!(f, "cannot format: {} syntax error(s)", d.len()),
            FormatError::Unsupported(d) => write!(f, "cannot format: {}", d.message),
            FormatError::Internal(m) => write!(f, "internal formatter error: {m}"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Format a source snapshot.
pub fn format(source: Vec<u8>, cfg: &FormatConfig) -> Result<Vec<u8>, FormatError> {
    format_parsed(&Parsed::new(source), cfg)
}

/// Format an already parsed snapshot.
pub fn format_parsed(parsed: &Parsed, cfg: &FormatConfig) -> Result<Vec<u8>, FormatError> {
    // A file with only comments makes the parser report a missing design unit, but there is
    // no code that could be damaged.
    if !parsed.errors.is_empty() && !parsed.is_blank() {
        return Err(FormatError::Syntax(parsed.errors.clone()));
    }
    if let Some(t) = parsed
        .tokens()
        .iter()
        .find(|t| matches!(t.kind(), TokenKind::ToolDirective | TokenKind::Unknown))
    {
        return Err(FormatError::Unsupported(Diagnostic {
            offset: t.text_offset(),
            message: "tool directives must be on a line of their own".into(),
        }));
    }
    let mut builder = format::Builder::new(cfg, parsed);
    let doc = builder.node(&parsed.root);
    let printed = Parsed::new(doc::print(doc, builder.groups(), &cfg.into()));
    verify::equivalent_parsed(parsed, &printed).map_err(FormatError::Internal)?;
    // Only changes the spaces between code and a trailing comment on the same line, which
    // cannot change the verified tokens and comments.
    let out = align::align_comments(printed, cfg);
    // Comment text only, after the code is laid out and verified.
    let out = reflow::comments(out, cfg);
    let mut out = parsed.restore_directives(out);
    let crlf = match cfg.line_ending {
        Some(ending) => ending == config::LineEnding::CrLf,
        None => parsed.uses_crlf(),
    };
    if crlf {
        out = to_crlf(&out);
    }
    Ok(out)
}

/// Starts of lines whose first element is a token or an own-line comment, keyed by position
/// in the token stream: `(token index, comment index or usize::MAX for the token)` → offset of
/// the line start and of the first non-blank character.
fn line_heads(parsed: &Parsed) -> HashMap<(usize, usize), (usize, usize)> {
    let src = parsed.source();
    let line_start = |offset: usize| {
        src[..offset]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1)
    };
    let starts_line = |offset: usize| {
        let start = line_start(offset);
        src[start..offset]
            .iter()
            .all(|b| matches!(b, b' ' | b'\t'))
            .then_some(start)
    };
    let mut out = HashMap::new();
    for (i, t) in parsed.tokens().iter().enumerate() {
        let trivia = t.leading_trivia();
        let len: usize = trivia.into_iter().map(TriviaPiece::byte_len).sum();
        let mut offset = t.text_offset() - len;
        let mut k = 0;
        for piece in trivia {
            if matches!(
                piece,
                TriviaPiece::LineComment(_) | TriviaPiece::BlockComment(_)
            ) {
                if let Some(start) = starts_line(offset) {
                    out.insert((i, k), (start, offset));
                }
                k += 1;
            }
            offset += piece.byte_len();
        }
        if t.kind() != TokenKind::Eof
            && let Some(start) = starts_line(t.text_offset())
        {
            out.insert((i, usize::MAX), (start, t.text_offset()));
        }
    }
    out
}

/// Re-indent a snapshot without changing anything else (VSG `--style indent_only`): each line
/// that starts with a token or comment gets the indentation that the token or comment has in
/// the formatted output, if it starts a line there too.
pub fn reindent(parsed: &Parsed, cfg: &FormatConfig) -> Result<Vec<u8>, FormatError> {
    let formatted = Parsed::new(format_parsed(parsed, cfg)?);
    let target = line_heads(&formatted);
    let mut edits: Vec<TextEdit> = line_heads(parsed)
        .into_iter()
        .filter_map(|(key, (start, first))| {
            let (fstart, ffirst) = target.get(&key)?;
            let indent = &formatted.source()[*fstart..*ffirst];
            (indent != &parsed.source()[start..first]).then(|| TextEdit {
                start,
                end: first,
                text: indent.to_vec(),
            })
        })
        .collect();
    edits.sort_by_key(|e| e.start);
    Ok(apply_edits(parsed.source(), &edits))
}

/// Replace `start..end` (byte offsets into the original source) with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start: usize,
    pub end: usize,
    pub text: Vec<u8>,
}

/// Apply sorted, non-overlapping edits to `source`.
pub fn apply_edits(source: &[u8], edits: &[TextEdit]) -> Vec<u8> {
    let mut out = Vec::with_capacity(source.len());
    let mut pos = 0;
    for e in edits {
        out.extend_from_slice(&source[pos..e.start]);
        out.extend_from_slice(&e.text);
        pos = e.end;
    }
    out.extend_from_slice(&source[pos..]);
    out
}

fn is_blank_line(line: &[u8]) -> bool {
    line.trim_ascii().is_empty()
}

/// Split a changed hunk into single-line changes and blank-line insertions/removals when its
/// non-blank lines correspond one to one; otherwise keep it whole.
fn split_hunk(
    old: &[&[u8]],
    new: &[&[u8]],
    o: &Range<usize>,
    n: &Range<usize>,
) -> Vec<(Range<usize>, Range<usize>)> {
    let old_code: Vec<usize> = o.clone().filter(|&i| !is_blank_line(old[i])).collect();
    let new_code: Vec<usize> = n.clone().filter(|&j| !is_blank_line(new[j])).collect();
    if old_code.len() != new_code.len() {
        return vec![(o.clone(), n.clone())];
    }
    let mut out = Vec::new();
    let (mut oi, mut nj) = (o.start, n.start);
    for (&i, &j) in old_code.iter().zip(&new_code).chain([(&o.end, &n.end)]) {
        // Blank lines in front of this pair of lines.
        if i - oi != j - nj {
            out.push((oi..i, nj..j));
        }
        if i < o.end && old[i] != new[j] {
            out.push((i..i + 1, j..j + 1));
        }
        oi = i + 1;
        nj = j + 1;
    }
    out
}

/// Format only the lines touched by the byte range `range` (expanded to whole lines).
///
/// The whole snapshot is formatted and diffed line by line against the source; the returned
/// edits are the changed hunks whose original lines intersect the range, in source order.
pub fn format_range(
    parsed: &Parsed,
    cfg: &FormatConfig,
    range: Range<usize>,
) -> Result<Vec<TextEdit>, FormatError> {
    let formatted = format_parsed(parsed, cfg)?;
    Ok(range_edits(parsed, &formatted, range, |edited| {
        verify::equivalent_parsed(parsed, &Parsed::new(edited.to_vec())).is_ok()
    }))
}

/// [`format_range`] for `vsg-rs fix`: fixes and formatting, limited to the lines in `range`.
pub fn fix_range(
    parsed: &Parsed,
    config: &Config,
    options: &FixOptions,
    range: Range<usize>,
) -> Result<Vec<TextEdit>, FormatError> {
    let fixed = fix_with(parsed, config, options)?.output;
    Ok(range_edits(parsed, &fixed, range, |edited| {
        Parsed::new(edited.to_vec()).syntax_errors().is_empty()
    }))
}

/// The line hunks between the source and `new` that touch `range`. `accept` checks a partial
/// result; if no partial result is accepted, all hunks are returned.
fn range_edits(
    parsed: &Parsed,
    new: &[u8],
    range: Range<usize>,
    accept: impl Fn(&[u8]) -> bool,
) -> Vec<TextEdit> {
    let src = parsed.source();
    let old: Vec<&[u8]> = src.split_inclusive(|&b| b == b'\n').collect();
    let new: Vec<&[u8]> = new.split_inclusive(|&b| b == b'\n').collect();
    // Start offset of every line; a final newline starts an empty line (index `old.len()`).
    let mut starts: Vec<usize> = old
        .iter()
        .scan(0, |pos, l| {
            let start = *pos;
            *pos += l.len();
            Some(start)
        })
        .collect();
    if src.is_empty() || src.ends_with(b"\n") {
        starts.push(src.len());
    }
    let line_of = |offset: usize| {
        starts
            .partition_point(|&s| s <= offset.min(src.len()))
            .saturating_sub(1)
    };
    let first = line_of(range.start);
    let last = line_of(range.end.saturating_sub(1)).max(first);
    let offset = |line: usize| starts.get(line).copied().unwrap_or(src.len());

    // Changed hunks as (old line range, new line range).
    let mut hunks: Vec<(Range<usize>, Range<usize>)> = Vec::new();
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, &old, &new) {
        let (tag, o, n) = op.as_tag_tuple();
        if tag == similar::DiffTag::Equal {
            continue;
        }
        if let Some(h) = hunks.last_mut()
            && h.0.end == o.start
            && h.1.end == n.start
        {
            h.0.end = o.end;
            h.1.end = n.end;
        } else {
            hunks.push((o, n));
        }
    }
    // Hunks are also tried line by line (blank lines as separate edits), so adjacent
    // statements stay separate.
    let fine: Vec<(Range<usize>, Range<usize>)> = hunks
        .iter()
        .flat_map(|(o, n)| split_hunk(&old, &new, o, n))
        .collect();
    let to_edit = |(o, n): &(Range<usize>, Range<usize>)| TextEdit {
        start: offset(o.start),
        end: offset(o.end),
        text: new[n.clone()].concat(),
    };
    let touched = |(o, _): &&(Range<usize>, Range<usize>)| {
        if o.is_empty() {
            (first..=last).contains(&o.start)
        } else {
            o.start <= last && o.end > first
        }
    };
    for candidate in [&fine, &hunks] {
        let edits: Vec<TextEdit> = candidate.iter().filter(touched).map(to_edit).collect();
        // Hunks are aligned on identical lines, which need not carry the same tokens (one
        // `end;` line can pair with another), so a partial result can lose tokens.
        if edits.is_empty() || accept(&apply_edits(src, &edits)) {
            return edits;
        }
    }
    // ponytail: last resort formats everything; grow the hunk set if this shows up in practice.
    hunks.iter().map(to_edit).collect()
}

fn to_crlf(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + text.len() / 16);
    for &b in text {
        if b == b'\n' {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blank-line rules would insert lines around the statements; keep the tests about ranges.
    fn cfg() -> FormatConfig {
        FormatConfig {
            blank: blank::BlankSettings::preserve(),
            ..FormatConfig::default()
        }
    }

    fn range_format(src: &str, range: Range<usize>) -> (Vec<TextEdit>, String) {
        let parsed = Parsed::new(src.as_bytes().to_vec());
        let edits = format_range(&parsed, &cfg(), range).unwrap();
        let out = apply_edits(parsed.source(), &edits);
        assert!(Parsed::new(out.clone()).syntax_errors().is_empty());
        (edits, String::from_utf8(out).unwrap())
    }

    const SRC: &str =
        "entity e is\nend;\narchitecture rtl of e is\nbegin\n  a<=b;\n  c<=d;\nend;\n";

    #[test]
    fn range_changes_only_the_selected_statement() {
        let at = SRC.find("c<=d").unwrap();
        let (edits, out) = range_format(SRC, at..at + 2);
        assert_eq!(edits.len(), 1);
        assert_eq!(out, SRC.replace("c<=d", "c <= d"));
    }

    #[test]
    fn range_on_formatted_lines_changes_nothing() {
        let (edits, _) = range_format(SRC, 0..0);
        assert!(edits.is_empty());
        let full = String::from_utf8(format(SRC.into(), &cfg()).unwrap()).unwrap();
        let (edits, out) = range_format(&full, 0..0);
        assert!(edits.is_empty());
        assert_eq!(out, full);
    }

    #[test]
    fn range_whole_file_equals_format() {
        let full = format(SRC.into(), &cfg()).unwrap();
        let (_, out) = range_format(SRC, 0..SRC.len());
        assert_eq!(out.as_bytes(), full);
    }

    #[test]
    fn range_preserves_crlf() {
        let src = SRC.replace('\n', "\r\n");
        let at = src.find("a<=b").unwrap();
        let (edits, out) = range_format(&src, at..at);
        assert_eq!(edits.len(), 1);
        assert_eq!(out, src.replace("a<=b", "a <= b"));
    }

    #[test]
    fn range_rejects_syntax_errors() {
        let parsed = Parsed::new(b"entity e is port (a : in bit; end;\n".to_vec());
        assert!(format_range(&parsed, &FormatConfig::default(), 0..1).is_err());
    }
}

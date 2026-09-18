//! Alignment policy (VSG `alignment` rules): declaration names, colons and `:=` in declarative
//! parts, `<=`/`:=` in consecutive assignments, and trailing comments.
//!
//! Operators and names are aligned by the layout builder with padding. Trailing comments are
//! aligned after printing, because their column depends on where the printed lines end: that
//! pass only changes the spaces in front of trailing comments.

use vhdl_syntax::syntax::{NodeKind as N, SyntaxNode, SyntaxToken};
use vhdl_syntax::tokens::{TokenKind, TriviaPiece};

use crate::Parsed;
use crate::config::{FormatConfig, Severity};
use crate::doc::display_width;
use crate::rules::RuleInfo;

/// One alignment policy and how its groups are delimited (VSG's yes/no options).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Family {
    pub enabled: bool,
    /// A blank line starts a new group.
    pub blank_line_ends_group: bool,
    /// A comment line starts a new group.
    pub comment_line_ends_group: bool,
    /// Trailing comments only: lines without a comment also count for the column.
    pub include_lines_without_comments: bool,
}

impl Family {
    const fn new(blank: bool, comment: bool, all_lines: bool) -> Family {
        Family {
            enabled: true,
            blank_line_ends_group: blank,
            comment_line_ends_group: comment,
            include_lines_without_comments: all_lines,
        }
    }
}

/// Alignment settings resolved from the configuration.
#[allow(clippy::struct_excessive_bools)] // One switch per VSG alignment rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignSettings {
    /// Names after the declaration keywords (`architecture_029`).
    pub declaration_names: Family,
    /// `:` of declarations (`architecture_026`).
    pub declaration_colons: Family,
    /// `:=` of declarations (`declarative_part_400`).
    pub declaration_assignments: Family,
    /// `<=` of consecutive concurrent assignments (`concurrent_006`).
    pub concurrent_assignments: Family,
    /// `<=` and `:=` of consecutive sequential assignments (`process_400`).
    pub sequential_assignments: Family,
    /// Trailing comments in declarative parts (`architecture_027`).
    pub declaration_comments: Family,
    /// Trailing comments in generic and port clauses (`entity_020`).
    pub interface_comments: Family,
    /// Trailing comments in generic and port maps (`instantiation_029`).
    pub map_comments: Family,
    /// Trailing comments of consecutive concurrent statements (`concurrent_008`).
    pub concurrent_comments: Family,
    /// Trailing comments in process bodies (`process_035`).
    pub process_comments: Family,
    /// `:` in the generic and port clauses of entities and blocks (`entity_017`).
    pub interface_colons: bool,
    /// `:` in the generic and port clauses of components (`component_017`).
    pub component_colons: bool,
    /// `:` in subprogram parameter lists (`procedure_410`).
    pub parameter_colons: bool,
    /// `=>` in generic and port maps and other named associations (`instantiation_010`).
    pub map_arrows: bool,
}

impl Default for AlignSettings {
    fn default() -> Self {
        let group = Family::new(true, true, false);
        AlignSettings {
            declaration_names: group,
            declaration_colons: group,
            declaration_assignments: group,
            concurrent_assignments: group,
            sequential_assignments: group,
            declaration_comments: group,
            interface_comments: group,
            map_comments: group,
            concurrent_comments: group,
            process_comments: Family::new(false, false, true),
            interface_colons: true,
            component_colons: true,
            parameter_colons: true,
            map_arrows: true,
        }
    }
}

const fn info(id: &'static str) -> RuleInfo {
    RuleInfo {
        id,
        groups: &["alignment"],
        severity: Severity::Error,
        enabled_by_default: true,
        description: "Alignment policy (applied by the formatter).",
    }
}

/// The VSG rules that configure each family, in [`AlignSettings::families_mut`] order.
pub(crate) static RULES: [RuleInfo; 10] = [
    info("architecture_029"),
    info("architecture_026"),
    info("declarative_part_400"),
    info("concurrent_006"),
    info("process_400"),
    info("architecture_027"),
    info("entity_020"),
    info("instantiation_029"),
    info("concurrent_008"),
    info("process_035"),
];

impl AlignSettings {
    pub(crate) fn families_mut(&mut self) -> [&mut Family; 10] {
        [
            &mut self.declaration_names,
            &mut self.declaration_colons,
            &mut self.declaration_assignments,
            &mut self.concurrent_assignments,
            &mut self.sequential_assignments,
            &mut self.declaration_comments,
            &mut self.interface_comments,
            &mut self.map_comments,
            &mut self.concurrent_comments,
            &mut self.process_comments,
        ]
    }

    fn comments_enabled(&self) -> bool {
        [
            self.declaration_comments,
            self.interface_comments,
            self.map_comments,
            self.concurrent_comments,
            self.process_comments,
        ]
        .iter()
        .any(|f| f.enabled)
    }
}

// Trailing comments ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Region {
    Declarations,
    Interface,
    Map,
    Concurrent,
    Process,
}

fn region_of(kind: N, parent: Option<N>) -> Option<Region> {
    use N::*;
    Some(match kind {
        ArchitectureDeclarativePart
        | BlockDeclarativePart
        | ProcessDeclarativePart
        | PackageDeclarativePart
        | PackageBodyDeclarativePart
        | SubprogramDeclarativePart
        | ProtectedTypeBodyDeclarativePart
        | ProtectedTypeDeclarativePart
        | EntityDeclarativePart => Region::Declarations,
        InterfaceList if matches!(parent, Some(GenericClause | PortClause)) => Region::Interface,
        AssociationList if matches!(parent, Some(GenericMapAspect | PortMapAspect)) => Region::Map,
        ArchitectureStatementPart | BlockStatementPart => Region::Concurrent,
        ProcessStatementPart => Region::Process,
        _ => return None,
    })
}

/// The innermost alignment region containing `t`.
fn innermost_region(t: &SyntaxToken) -> Option<(Region, SyntaxNode)> {
    let mut node = Some(t.parent());
    while let Some(n) = node {
        let parent = n.parent();
        if let Some(r) = region_of(n.kind(), parent.as_ref().map(SyntaxNode::kind)) {
            return Some((r, n));
        }
        node = parent;
    }
    None
}

struct Line {
    start: usize,
    /// End of the code (exclusive): before the trailing comment, or the end of the line.
    code_end: usize,
    blank: bool,
    comment_only: bool,
    /// Start of the trailing comment, if any (the spaces before it start at `code_end`).
    comment: Option<usize>,
    /// Region (by node offset) of the first token of the line.
    region: Option<usize>,
    /// Region (by node offset) of the token the trailing comment follows.
    comment_region: Option<usize>,
    /// Inside a `fmt off` region.
    disabled: bool,
}

/// Width of `text`, with leading tabs (smart-tab indentation) counted as `tab` columns.
fn width(text: &[u8], utf8: bool, tab: usize) -> usize {
    let tabs = text.iter().take_while(|&&b| b == b'\t').count();
    display_width(&text[tabs..], utf8, tabs * tab)
}

/// Align trailing comments in `parsed`, the printed output (`\n` line endings).
pub(crate) fn align_comments(parsed: Parsed, cfg: &FormatConfig) -> Vec<u8> {
    if !cfg.align.comments_enabled() || !parsed.errors.is_empty() {
        return parsed.source;
    }
    let source = parsed.source();
    let utf8 = parsed.is_utf8();
    let mut line_starts = vec![0];
    line_starts.extend(
        source
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b == b'\n')
            .map(|(i, _)| i + 1),
    );
    let line_of = |offset: usize| line_starts.partition_point(|&s| s <= offset) - 1;
    let mut lines: Vec<Line> = line_starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = line_starts.get(i + 1).map_or(source.len(), |e| e - 1);
            let text = &source[start..end];
            let first = text.iter().position(|b| !matches!(b, b' ' | b'\t'));
            Line {
                start,
                code_end: end,
                blank: first.is_none(),
                comment_only: first.is_some_and(|p| text[p..].starts_with(b"--")),
                comment: None,
                region: None,
                comment_region: None,
                disabled: false,
            }
        })
        .collect();
    let tokens = parsed.tokens();
    let directives = crate::format::directives(&parsed);
    // Pass 1 (cheap): trailing comments, first token and `fmt off` state of each line.
    let mut first_token: Vec<Option<usize>> = vec![None; lines.len()];
    let mut commented: Vec<(usize, usize)> = Vec::new(); // (line, token index)
    for (i, t) in tokens.iter().enumerate() {
        if t.kind() == TokenKind::Eof {
            break;
        }
        let line = line_of(t.text_offset());
        lines[line].disabled |= crate::format::disabled_at(&directives, t.text_offset());
        first_token[line].get_or_insert(i);
        let Some(next) = tokens.get(i + 1) else {
            continue;
        };
        // A trailing comment: a line comment before the first newline of the next trivia.
        let mut offset = t.text_range().end;
        for piece in next.leading_trivia() {
            match piece {
                TriviaPiece::LineComment(_) => {
                    lines[line].comment = Some(offset);
                    lines[line].code_end = t.text_range().end;
                    commented.push((line, i));
                    break;
                }
                p if p.is_newline() => break,
                p => offset += p.byte_len(),
            }
        }
    }
    if commented.is_empty() {
        return parsed.source;
    }
    // Pass 2: regions of the commented lines; other lines are classified only inside those.
    let mut regions: Vec<(usize, Region, SyntaxNode)> = Vec::new();
    let mut region_key = |t: &SyntaxToken| {
        innermost_region(t).map(|(r, n)| {
            let key = n.offset();
            if !regions.iter().any(|(k, _, _)| *k == key) {
                regions.push((key, r, n));
            }
            key
        })
    };
    for &(line, i) in &commented {
        lines[line].comment_region = region_key(&tokens[i]);
    }
    let spans: Vec<std::ops::Range<usize>> =
        regions.iter().map(|(_, _, n)| n.text_range()).collect();
    for range in spans {
        for l in line_of(range.start)..=line_of(range.end.saturating_sub(1)) {
            let line = &lines[l];
            if line.region.is_some() || line.blank || line.comment_only || line.disabled {
                continue;
            }
            if let Some(i) = first_token[l] {
                // Only the innermost region matters; nested regions are resolved the same way.
                lines[l].region = innermost_region(&tokens[i]).map(|(_, n)| n.offset());
            }
        }
    }
    let col = |l: &Line| width(&source[l.start..l.code_end], utf8, cfg.indent);
    // (start of the spaces before a comment, start of the comment, target column)
    let mut edits: Vec<(usize, usize, usize)> = Vec::new();
    for (key, region, node) in &regions {
        let family = match region {
            Region::Declarations => cfg.align.declaration_comments,
            Region::Interface => cfg.align.interface_comments,
            Region::Map => cfg.align.map_comments,
            Region::Concurrent => cfg.align.concurrent_comments,
            Region::Process => cfg.align.process_comments,
        };
        if !family.enabled {
            continue;
        }
        let range = node.text_range();
        let mut group: Vec<usize> = Vec::new();
        let mut flush = |group: &mut Vec<usize>| {
            let mine = |l: &&Line| l.comment.is_some() && l.comment_region == Some(*key);
            let members = group.iter().map(|&l| &lines[l]);
            let max = if family.include_lines_without_comments {
                members.map(col).max()
            } else {
                members.filter(mine).map(col).max()
            };
            if let Some(max) = max {
                for l in group.iter().map(|&l| &lines[l]).filter(mine) {
                    if let Some(comment) = l.comment {
                        edits.push((l.code_end, comment, max + 1));
                    }
                }
            }
            group.clear();
        };
        let first = line_of(range.start);
        let last = line_of(range.end.saturating_sub(1));
        for (l, line) in lines.iter().enumerate().take(last + 1).skip(first) {
            let ends = (line.blank && family.blank_line_ends_group)
                || (line.comment_only && family.comment_line_ends_group)
                || line.disabled;
            if ends {
                flush(&mut group);
            } else if line.blank || line.comment_only {
            } else if line.region == Some(*key)
                || line.comment_region == Some(*key)
                || family.include_lines_without_comments
            {
                group.push(l);
            } else {
                // A nested construct separates groups.
                flush(&mut group);
            }
        }
        flush(&mut group);
    }
    if edits.is_empty() {
        return parsed.source;
    }
    edits.sort_unstable();
    edits.dedup_by_key(|(s, _, _)| *s);
    let mut result = Vec::with_capacity(source.len() + edits.len() * 8);
    let mut pos = 0;
    for (spaces, comment, column) in edits {
        let code = width(
            &source[line_starts[line_of(spaces)]..spaces],
            utf8,
            cfg.indent,
        );
        result.extend_from_slice(&source[pos..spaces]);
        result.extend(std::iter::repeat_n(
            b' ',
            column.saturating_sub(code).max(1),
        ));
        pos = comment;
    }
    result.extend_from_slice(&source[pos..]);
    result
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    fn fmt(src: &str, yaml: &str) -> String {
        let cfg = Config::parse(yaml).unwrap();
        String::from_utf8(crate::format(src.as_bytes().to_vec(), &cfg.format).unwrap()).unwrap()
    }

    const SRC: &str = "architecture rtl of e is\n  signal a : bit; -- first\n  constant long_name : integer := 1; -- second\n  signal b : bit := '0';\n\n  signal after_blank : bit;\nbegin\n  a <= b; -- one\n  long_name_signal <= c; -- two\n  process is\n    variable v : integer;\n  begin\n    v := 1; -- x\n    long_variable <= 2;\n    wait;\n  end process;\nend architecture rtl;\n";

    #[test]
    fn declarations_statements_and_comments() {
        let expected = "architecture rtl of e is\n\n  signal   a         : bit;          -- first\n  constant long_name : integer := 1; -- second\n  signal   b         : bit     := '0';\n\n  signal after_blank : bit;\n\nbegin\n\n  a                <= b; -- one\n  long_name_signal <= c; -- two\n\n  process is\n\n    variable v : integer;\n\n  begin\n\n    v             := 1; -- x\n    long_variable <= 2;\n    wait;\n\n  end process;\n\nend architecture rtl;\n";
        assert_eq!(fmt(SRC, ""), expected);
        assert_eq!(fmt(expected, ""), expected);
    }

    #[test]
    fn alignment_can_be_disabled() {
        let out = fmt(
            SRC,
            "rule:\n  group:\n    alignment:\n      disable: true\n",
        );
        assert!(
            out.contains(
                "  signal a : bit; -- first\n  constant long_name : integer := 1; -- second\n"
            ),
            "{out}"
        );
        assert!(out.contains("  a <= b; -- one\n"), "{out}");
    }
}

//! Sorting the context clauses: `library` and `use`, in a settled order.
//!
//! VSG has no rule about the order of a context clause and neither does vsg-rs, because there is
//! no order the language prefers: every one of them analyses. It is a convention, and a
//! convention every file in a project follows is worth something even though no file that breaks
//! it is wrong. That makes this an editor action rather than a rule, the same shape as "organize
//! imports" in other languages, and what LSP's `source.organizeImports` is for.
//!
//! The order is `ieee` and `std` first, then everything else alphabetically, then `work` last:
//! the standard libraries a reader takes for granted, the third-party ones they may not, and the
//! project's own at the end where it is easy to find.
//!
//! # Whole lines, never rewritten
//!
//! This moves blocks of lines and nothing else. A clause's text is never rebuilt from the tree,
//! so a comment, an alignment or a spelling cannot be lost on the way: the output is a
//! permutation of the input's lines, which this module checks before returning it. Anything that
//! does not fit that shape, such as two clauses sharing a line, means no action is offered at
//! all. A reordering that damages a file is worse than no reordering, and an editor action gets
//! no second look from the output check that guards `--fix`.

use std::collections::BTreeMap;

use vhdl_syntax::syntax::{NodeKind, SyntaxNode};

use crate::Parsed;
use crate::analysis::design::{find, text_of};

/// Where a library sorts: the standard ones first, `work` last, the rest alphabetically between.
fn rank(library: &str) -> (u8, String) {
    match library {
        "ieee" | "std" => (0, library.to_owned()),
        "work" => (2, library.to_owned()),
        other => (1, other.to_owned()),
    }
}

/// One library and the clauses belonging to it, as line ranges into the source.
struct Group {
    library: String,
    /// The `library` clause, when the group has one. A `use work.x.all` with no `library work;`
    /// above it is a group without one.
    clause: Option<(usize, usize)>,
    /// Each `use` clause, by the name it uses and the lines it occupies.
    uses: Vec<(String, (usize, usize))>,
}

/// The library a name refers to: `ieee.std_logic_1164.all` is `ieee`.
fn library_of(name: &str) -> String {
    name.split('.')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase()
}

/// The lines a node occupies, extended to whole lines and to a comment directly above it.
///
/// `None` when the node shares its first or last line with something that is not part of it,
/// because moving that line would move the other thing with it.
fn block(
    parsed: &Parsed,
    node: &SyntaxNode,
    lines: &[&str],
    taken: &[bool],
) -> Option<(usize, usize)> {
    let range = node.text_range();
    let (first, start_column) = parsed.line_col(range.start);
    let (last, end_column) = parsed.line_col(range.end.saturating_sub(1));
    let (mut first, last) = (first.checked_sub(1)?, last.checked_sub(1)?);

    // Anything before the clause on its first line, or after it on its last, is someone else's.
    let head = lines.get(first)?;
    if !head[..start_column.saturating_sub(1).min(head.len())]
        .trim()
        .is_empty()
    {
        return None;
    }
    let tail = lines.get(last)?;
    let rest = &tail[end_column.min(tail.len())..];
    if !rest.trim().is_empty() && !rest.trim_start().starts_with("--") {
        return None;
    }

    // A comment line directly above, with no blank line between, belongs to this clause.
    while first > 0 && !taken[first - 1] && lines[first - 1].trim_start().starts_with("--") {
        first -= 1;
    }
    Some((first, last))
}

/// Every context clause in a source, sorted, or `None` when there is nothing safe to do.
///
/// `None` covers every uncertain case: no context clause, one already in order, a clause sharing
/// a line with another, or a comment this cannot attribute to a clause.
#[must_use]
pub fn sort_context_clauses(parsed: &Parsed) -> Option<String> {
    let source = std::str::from_utf8(parsed.source()).ok()?;
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut out = source.to_owned();
    let mut changed = false;

    // Last one first: replacing an earlier region would move every later one.
    let mut clauses = find(parsed.root(), NodeKind::ContextClause);
    clauses.reverse();
    for context in clauses {
        let Some((from, to, text)) = sort_one(parsed, &context, &lines) else {
            continue;
        };
        let start: usize = lines[..from].iter().map(|line| line.len()).sum();
        let end: usize = lines[..=to].iter().map(|line| line.len()).sum();
        if out.get(start..end) == Some(text.as_str()) {
            continue;
        }
        out.replace_range(start..end, &text);
        changed = true;
    }
    changed.then_some(out)
}

/// One context clause, as the region it covers and the lines it should become.
fn sort_one(
    parsed: &Parsed,
    context: &SyntaxNode,
    lines: &[&str],
) -> Option<(usize, usize, String)> {
    let mut groups: Vec<Group> = Vec::new();
    let mut taken = vec![false; lines.len()];
    let mut first = usize::MAX;
    let mut last = 0;

    for item in context.children() {
        let (from, to) = block(parsed, &item, lines, &taken)?;
        for line in &mut taken[from..=to] {
            *line = true;
        }
        first = first.min(from);
        last = last.max(to);
        match item.kind() {
            NodeKind::LibraryClause => {
                // `library ieee, work;` names two libraries at once. Separating them would be
                // rewriting rather than moving, so the whole context clause is left alone.
                let text = text_of(&item);
                let named = text
                    .trim_start_matches("library")
                    .trim_end_matches(';')
                    .trim();
                if named.contains(',') {
                    return None;
                }
                groups.push(Group {
                    library: named.to_ascii_lowercase(),
                    clause: Some((from, to)),
                    uses: Vec::new(),
                });
            }
            NodeKind::UseClauseContextItem => {
                let text = text_of(&item);
                let used = text.trim_start_matches("use").trim_end_matches(';').trim();
                if used.contains(',') {
                    return None;
                }
                let library = library_of(used);
                match groups.iter_mut().find(|g| g.library == library) {
                    Some(group) => group.uses.push((used.to_owned(), (from, to))),
                    // `use work.pkg.all` with no `library work;` of its own: a group all the
                    // same, sorted by the same name.
                    None => groups.push(Group {
                        library,
                        clause: None,
                        uses: vec![(used.to_owned(), (from, to))],
                    }),
                }
            }
            // A context reference (`context work.c;`) or anything else: not something this
            // orders, and leaving it still while its neighbours move would be a guess.
            _ => return None,
        }
    }
    // One library with one use clause has no order to be in.
    if groups.len() < 2 && groups.iter().all(|group| group.uses.len() < 2) {
        return None;
    }
    // Every line in the region has to belong to a clause. A comment this could not attribute to
    // one would be left behind while the clauses moved around it.
    if (first..=last).any(|line| !taken[line] && !lines[line].trim().is_empty()) {
        return None;
    }

    groups.sort_by_key(|group| rank(&group.library));
    let mut text = String::new();
    for (at, group) in groups.iter_mut().enumerate() {
        if at > 0 {
            text.push('\n');
        }
        group.uses.sort_by(|a, b| a.0.cmp(&b.0));
        for (from, to) in group
            .clause
            .iter()
            .chain(group.uses.iter().map(|(_, at)| at))
        {
            for line in &lines[*from..=*to] {
                text.push_str(line);
            }
        }
    }

    // The result must be a permutation of the region's own lines, blank lines aside. Anything
    // else is a bug here, and someone's source is not the place to discover it.
    let count = |text: &mut dyn Iterator<Item = &str>| {
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for line in text.filter(|line| !line.trim().is_empty()) {
            *seen.entry(line.trim_end().to_owned()).or_default() += 1;
        }
        seen
    };
    let was = count(&mut lines[first..=last].iter().copied());
    let is = count(&mut text.split_inclusive('\n'));
    (was == is).then_some((first, last, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(source: &str) -> Option<String> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        sort_context_clauses(&parsed)
    }

    const ENTITY: &str = "\nentity dut is\nend entity dut;\n";

    #[test]
    fn ieee_first_then_alphabetical_then_work() {
        let source = format!(
            "library work;\nuse work.pkg.all;\n\nlibrary osvvm;\nuse osvvm.randompkg.all;\n\n\
             library ieee;\nuse ieee.std_logic_1164.all;\n{ENTITY}"
        );
        let wanted = format!(
            "library ieee;\nuse ieee.std_logic_1164.all;\n\nlibrary osvvm;\n\
             use osvvm.randompkg.all;\n\nlibrary work;\nuse work.pkg.all;\n{ENTITY}"
        );
        assert_eq!(sorted(&source).expect("a reordering"), wanted);
    }

    #[test]
    fn use_clauses_sort_within_their_library() {
        let source = format!(
            "library ieee;\nuse ieee.numeric_std.all;\nuse ieee.std_logic_1164.all;\n\
             use ieee.fixed_pkg.all;\n{ENTITY}"
        );
        let got = sorted(&source).expect("a reordering");
        assert!(
            got.starts_with(
                "library ieee;\nuse ieee.fixed_pkg.all;\nuse ieee.numeric_std.all;\n\
                 use ieee.std_logic_1164.all;\n"
            ),
            "{got}"
        );
    }

    #[test]
    fn a_comment_travels_with_its_clause() {
        let source = format!(
            "library work;\nuse work.pkg.all;\n\n-- the fixed-point package\nlibrary ieee;\n\
             use ieee.fixed_pkg.all;\n{ENTITY}"
        );
        let got = sorted(&source).expect("a reordering");
        assert!(
            got.starts_with("-- the fixed-point package\nlibrary ieee;\nuse ieee.fixed_pkg.all;"),
            "{got}"
        );
    }

    #[test]
    fn a_trailing_comment_is_kept() {
        let source = format!(
            "library work;\nuse work.pkg.all;\n\nlibrary ieee;  -- standard\n\
             use ieee.std_logic_1164.all;\n{ENTITY}"
        );
        let got = sorted(&source).expect("a reordering");
        assert!(got.contains("library ieee;  -- standard\n"), "{got}");
    }

    #[test]
    fn source_already_in_order_offers_nothing() {
        let source = format!(
            "library ieee;\nuse ieee.std_logic_1164.all;\n\nlibrary work;\n\
             use work.pkg.all;\n{ENTITY}"
        );
        assert_eq!(sorted(&source), None);
    }

    /// Two clauses on one line cannot be moved without rewriting them.
    #[test]
    fn clauses_sharing_a_line_are_left_alone() {
        let source = format!("library work; library ieee;\nuse ieee.numeric_std.all;\n{ENTITY}");
        assert_eq!(sorted(&source), None);
    }

    #[test]
    fn a_library_clause_naming_two_is_left_alone() {
        let source = format!("library work, ieee;\nuse ieee.std_logic_1164.all;\n{ENTITY}");
        assert_eq!(sorted(&source), None);
    }

    #[test]
    fn nothing_to_sort_offers_nothing() {
        assert_eq!(sorted(&format!("library ieee;\n{ENTITY}")), None);
        assert_eq!(sorted(ENTITY), None);
    }
}

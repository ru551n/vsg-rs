//! Design checks the analyser does not do: the two that open every RTL lint checklist.
//!
//! Both work on the syntax tree, because neither needs a type: a latch is inferred from *where*
//! a signal is assigned, and two drivers are two concurrent statements assigning the same name.
//! They run in the lint layer (`--check lint`) with the rest of the design rules.
//!
//! * `lint_600` — a combinational process assigns a signal on some paths but not all, so the
//!   signal has to remember its value: a latch. The classic causes are an `if` with no `else`
//!   and a `case` alternative that skips a target the others assign.
//! * `lint_601` — a signal is assigned by more than one concurrent statement. Legal for a
//!   resolved type and occasionally deliberate, wrong nearly every other time.

use std::collections::{BTreeMap, BTreeSet};

use vhdl_syntax::syntax::{NodeKind, SyntaxNode};
use vsg_rs::Parsed;

use crate::lint::Finding;

fn descendants(node: &SyntaxNode, kind: NodeKind, out: &mut Vec<SyntaxNode>) {
    for child in node.children() {
        if child.kind() == kind {
            out.push(child.clone());
        }
        descendants(&child, kind, out);
    }
}

fn find(node: &SyntaxNode, kind: NodeKind) -> Vec<SyntaxNode> {
    let mut out = Vec::new();
    descendants(node, kind, &mut out);
    out
}

/// Every token under a node, in order. `SyntaxNode::tokens` only yields the direct ones, and a
/// name is several levels deep.
fn tokens(node: &SyntaxNode, out: &mut Vec<vhdl_syntax::syntax::SyntaxToken>) {
    for element in node.children_with_tokens() {
        match element {
            vhdl_syntax::syntax::SyntaxElement::Node(n) => tokens(&n, out),
            vhdl_syntax::syntax::SyntaxElement::Token(t) => out.push(t),
        }
    }
}

fn all_tokens(node: &SyntaxNode) -> Vec<vhdl_syntax::syntax::SyntaxToken> {
    let mut out = Vec::new();
    tokens(node, &mut out);
    out
}

/// The text of a node, lowercased, with the trivia dropped: enough to compare two assignment
/// targets without resolving either.
fn text_of(node: &SyntaxNode) -> String {
    all_tokens(node)
        .iter()
        .map(|t| String::from_utf8_lossy(t.text().as_bytes()).to_ascii_lowercase())
        .collect()
}

/// The name a signal assignment writes to, as written.
fn target(assignment: &SyntaxNode) -> Option<(String, usize)> {
    let target = assignment
        .children()
        .find(|c| c.kind() == NodeKind::NameTarget)?;
    let offset = all_tokens(&target).first()?.text_offset();
    let text = text_of(&target);
    // The object, not the part of it: `q(3)` and `q(4)` drive the same signal.
    let base = text.split(['(', '.']).next().unwrap_or(&text).to_owned();
    (!base.is_empty()).then_some((base, offset))
}

const ASSIGNMENTS: &[NodeKind] = &[
    NodeKind::ConcurrentSimpleSignalAssignment,
    NodeKind::ConcurrentConditionalSignalAssignment,
    NodeKind::ConcurrentSelectedSignalAssignment,
    NodeKind::SimpleWaveformAssignment,
    NodeKind::ConditionalWaveformAssignment,
    NodeKind::SelectedWaveformAssignment,
    NodeKind::SimpleForceAssignment,
    NodeKind::ConditionalForceAssignment,
];

/// Every signal assignment in a node, the node itself included, as (signal, offset).
fn assignments(node: &SyntaxNode) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    if ASSIGNMENTS.contains(&node.kind()) {
        out.extend(target(node));
    }
    for kind in ASSIGNMENTS {
        for assignment in find(node, *kind) {
            out.extend(target(&assignment));
        }
    }
    out
}

/// Whether a process describes something other than combinational logic, and so cannot infer a
/// latch: a clocked process (an edge test, the shape `vhdl_lang` uses too), or one that suspends
/// on `wait`, which is how testbenches are written and is not synthesisable logic at all.
fn is_not_combinational(process: &SyntaxNode) -> bool {
    if !find(process, NodeKind::WaitStatement).is_empty() {
        return true;
    }
    let text = text_of(process);
    text.contains("rising_edge(") || text.contains("falling_edge(") || text.contains("'event")
}

/// A latch needs a signal that some path assigns and another path does not. Only the shapes that
/// say so without ambiguity are reported: an `if` with no `else` branch, and a `case` whose
/// alternatives do not all assign the same signals.
fn latches(process: &SyntaxNode) -> Vec<(String, usize)> {
    // Signals the process assigns outside any branch always have a value, whatever follows.
    // A process holds its statements directly; an `if` wraps its own in a `SequenceOfStatements`.
    let mut unconditional: BTreeSet<String> = BTreeSet::new();
    for part in find(process, NodeKind::ProcessStatementPart) {
        for statement in part.children() {
            if matches!(
                statement.kind(),
                NodeKind::IfStatement | NodeKind::CaseStatement
            ) {
                continue;
            }
            unconditional.extend(assignments(&statement).into_iter().map(|(name, _)| name));
        }
    }

    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for statement in find(process, NodeKind::IfStatement) {
        if statement
            .children()
            .any(|c| c.kind() == NodeKind::IfStatementElse)
        {
            continue;
        }
        for (name, offset) in assignments(&statement) {
            if !unconditional.contains(&name) {
                out.entry(name).or_insert(offset);
            }
        }
    }
    for statement in find(process, NodeKind::CaseStatement) {
        let alternatives: Vec<BTreeSet<String>> =
            find(&statement, NodeKind::CaseStatementAlternative)
                .iter()
                .map(|a| assignments(a).into_iter().map(|(name, _)| name).collect())
                .collect();
        let Some(first) = alternatives.first() else {
            continue;
        };
        let everywhere = alternatives
            .iter()
            .skip(1)
            .fold(first.clone(), |acc, set| &acc & set);
        for (name, offset) in assignments(&statement) {
            if !unconditional.contains(&name) && !everywhere.contains(&name) {
                out.entry(name).or_insert(offset);
            }
        }
    }
    out.into_iter().collect()
}

pub(crate) fn check(parsed: &Parsed, file: &std::path::Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let at = |offset: usize| parsed.line_col(offset);

    for architecture in find(parsed.root(), NodeKind::ArchitectureBody) {
        // lint_601: one name, assigned by two concurrent statements.
        let mut drivers: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for statement in find(&architecture, NodeKind::ArchitectureStatementPart)
            .iter()
            .flat_map(SyntaxNode::children)
        {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            for (name, offset) in assignments(&statement) {
                if seen.insert(name.clone()) {
                    drivers.entry(name).or_default().push(offset);
                }
            }
        }
        for (name, offsets) in drivers {
            if offsets.len() < 2 {
                continue;
            }
            let lines: Vec<String> = offsets.iter().map(|o| at(*o).0.to_string()).collect();
            let (line, column) = at(offsets[0]);
            findings.push(Finding {
                file: file.to_path_buf(),
                rule: "lint_601",
                line,
                column,
                message: format!(
                    "Signal '{name}' is assigned by {} concurrent statements (lines {})",
                    offsets.len(),
                    lines.join(", ")
                ),
            });
        }

        // lint_600: a combinational process that does not assign on every path.
        for process in find(&architecture, NodeKind::ProcessStatement) {
            if is_not_combinational(&process) {
                continue;
            }
            for (name, offset) in latches(&process) {
                let (line, column) = at(offset);
                findings.push(Finding {
                    file: file.to_path_buf(),
                    rule: "lint_600",
                    line,
                    column,
                    message: format!(
                        "Signal '{name}' is not assigned on every path of this combinational \
                         process, which infers a latch"
                    ),
                });
            }
        }
    }
    findings.sort_by_key(|f| (f.line, f.column, f.rule));
    findings
}

/// The rules this module reports, for `--list_rules`.
pub(crate) const RULES: &[(&str, &str)] = &[
    (
        "lint_600",
        "A combinational process does not assign a signal on every path, inferring a latch.",
    ),
    (
        "lint_601",
        "A signal is assigned by more than one concurrent statement.",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn check_source(text: &str) -> Vec<(&'static str, usize)> {
        let parsed = Parsed::new(text.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        check(&parsed, std::path::Path::new("dut.vhd"))
            .into_iter()
            .map(|f| (f.rule, f.line))
            .collect()
    }

    const PREAMBLE: &str = "entity dut is\nend entity;\n\narchitecture rtl of dut is\n  \
                            signal a, b, c, d, clk : bit;\nbegin\n";

    #[test]
    fn an_if_without_else_infers_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a, b) is\n  begin\n    if a = '1' then\n      \
             c <= b;\n    end if;\n  end process;\nend architecture;\n"
        ));
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_600"),
            "got {found:?}"
        );
    }

    #[test]
    fn a_default_assignment_prevents_the_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a, b) is\n  begin\n    c <= '0';\n    if a = '1' then\n      \
             c <= b;\n    end if;\n  end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "an unconditional assignment is a default, got {found:?}"
        );
    }

    #[test]
    fn a_process_that_waits_is_not_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process is\n  begin\n    wait until a = '1';\n    if b = '1' then\n      \
             c <= b;\n    end if;\n    wait;\n  end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "a testbench process is not combinational logic, got {found:?}"
        );
    }

    #[test]
    fn a_clocked_process_is_not_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (clk) is\n  begin\n    if rising_edge(clk) then\n      \
             c <= b;\n    end if;\n  end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "a register is not a latch, got {found:?}"
        );
    }

    #[test]
    fn two_processes_driving_one_signal() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a) is\n  begin\n    c <= a;\n  end process;\n\n  \
             q : process (b) is\n  begin\n    c <= b;\n  end process;\nend architecture;\n"
        ));
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_601"),
            "got {found:?}"
        );
    }

    #[test]
    fn a_concurrent_assignment_counts_as_a_driver() {
        let found = check_source(&format!(
            "{PREAMBLE}  c <= a;\n\n  p : process (b) is\n  begin\n    c <= b;\n  \
             end process;\nend architecture;\n"
        ));
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_601"),
            "got {found:?}"
        );
    }

    #[test]
    fn one_driver_is_not_reported() {
        let found = check_source(&format!(
            "{PREAMBLE}  c <= a;\n  d <= b;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_601"),
            "got {found:?}"
        );
    }
}

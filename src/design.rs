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

/// The signals a sequence of statements assigns on *every* path through it.
///
/// A branch that cannot be shown to assign contributes nothing: an `if` without `else`, a `case`
/// without `others`, a loop that may run zero times. That is what makes the difference between a
/// default assignment (no latch) and a conditional one (latch).
fn always_assigned(statements: &[SyntaxNode]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for statement in statements {
        match statement.kind() {
            NodeKind::IfStatement => {
                let Some(otherwise) = statement
                    .children()
                    .find(|c| c.kind() == NodeKind::IfStatementElse)
                else {
                    continue; // No `else`: some path assigns nothing.
                };
                let mut branches: Vec<BTreeSet<String>> = vec![always_assigned(&body(statement))];
                branches.extend(
                    statement
                        .children()
                        .filter(|c| c.kind() == NodeKind::IfStatementElsif)
                        .map(|elsif| always_assigned(&body(&elsif))),
                );
                branches.push(always_assigned(&body(&otherwise)));
                out.extend(intersection(branches));
            }
            NodeKind::CaseStatement => {
                let alternatives: Vec<SyntaxNode> = statement
                    .children()
                    .filter(|c| c.kind() == NodeKind::CaseStatementAlternative)
                    .collect();
                // A case must cover its type, but only `others` says so without knowing the type.
                let covers_everything = alternatives
                    .iter()
                    .any(|a| text_of(a).starts_with("whenothers"));
                if !covers_everything || alternatives.is_empty() {
                    continue;
                }
                out.extend(intersection(
                    alternatives
                        .iter()
                        .map(|a| always_assigned(&body(a)))
                        .collect(),
                ));
            }
            // A `for` loop over a range runs, and synthesis unrolls it, so its body assigns.
            // A `while` loop may not run at all, so it promises nothing.
            NodeKind::LoopStatement
                if statement
                    .children()
                    .find(|c| c.kind() == NodeKind::LoopStatementPreamble)
                    .is_some_and(|p| text_of(&p).starts_with("for")) =>
            {
                out.extend(always_assigned(&body(statement)));
            }
            kind if ASSIGNMENTS.contains(&kind) => {
                out.extend(target(statement).map(|(name, _)| name));
            }
            _ => {}
        }
    }
    out
}

fn intersection(sets: Vec<BTreeSet<String>>) -> BTreeSet<String> {
    let mut iter = sets.into_iter();
    let first = iter.next().unwrap_or_default();
    iter.fold(first, |acc, set| &acc & &set)
}

/// The statements a node holds: those of its `SequenceOfStatements`, or its own children when it
/// keeps them directly, as a process does.
fn body(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let sequences: Vec<SyntaxNode> = node
        .children()
        .filter(|c| c.kind() == NodeKind::SequenceOfStatements)
        .collect();
    if sequences.is_empty() {
        return node.children().collect();
    }
    sequences.iter().flat_map(SyntaxNode::children).collect()
}

/// Signals a process assigns somewhere but not on every path: each has to remember its value.
fn latches(process: &SyntaxNode) -> Vec<(String, usize)> {
    let statements: Vec<SyntaxNode> = find(process, NodeKind::ProcessStatementPart)
        .iter()
        .flat_map(body)
        .collect();
    let always = always_assigned(&statements);
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for (name, offset) in assignments(process) {
        if !always.contains(&name) {
            out.entry(name).or_insert(offset);
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
    fn a_default_inside_a_branch_prevents_the_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a, b) is\n  begin\n    if a = '1' then\n      \
             d <= '0';\n      if b = '1' then\n        d <= '1';\n      end if;\n    else\n      \
             d <= '0';\n    end if;\n  end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "every path assigns d, got {found:?}"
        );
    }

    #[test]
    fn a_nested_if_without_else_is_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a, b) is\n  begin\n    if a = '1' then\n      \
             c <= '1';\n      if b = '1' then\n        d <= '1';\n      end if;\n    else\n      \
             c <= '0';\n      d <= '0';\n    end if;\n  end process;\nend architecture;\n"
        ));
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_600"),
            "a = '1' with b = '0' leaves d unassigned, got {found:?}"
        );
    }

    #[test]
    fn a_case_with_others_assigning_everywhere_is_not_a_latch() {
        let found = check_source(&format!(
            "{PREAMBLE}  p : process (a) is\n  begin\n    case a is\n      when '0' =>\n        \
             c <= '0';\n      when others =>\n        c <= '1';\n    end case;\n  \
             end process;\nend architecture;\n"
        ));
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "every alternative assigns c, got {found:?}"
        );
    }

    #[test]
    fn a_for_loop_assigns_every_element() {
        let found = check_source(
            "entity dut is\nend entity;\n\narchitecture rtl of dut is\n  \
             signal a, b : bit_vector(3 downto 0);\nbegin\n  p : process (a) is\n  begin\n    \
             for i in a'range loop\n      b(i) <= a(i);\n    end loop;\n  end process;\n\
             end architecture;\n",
        );
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_600"),
            "the loop covers every bit, got {found:?}"
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

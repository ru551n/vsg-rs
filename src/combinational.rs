//! Combinational loops: logic that feeds itself without a register in the way.
//!
//! `lint_720` builds the dependency graph of one architecture — every signal a combinational
//! source reads, pointing at every signal it drives — and looks for a cycle. A clocked process
//! contributes no edges, because the register is exactly what breaks the loop.
//!
//! Hardware that does this oscillates, or settles somewhere nobody chose, and it is one of the
//! findings a netlist-level tool is usually thought necessary for. It is not: the cycle is in the
//! source, as long as the graph is built only from what can be read with certainty.
//!
//! Deliberately left out, so that a reported cycle is a real one:
//!
//! * assignments to part of an object (`q(0) <= q(1)`), where the whole object would look as
//!   though it fed itself when two different elements are involved;
//! * anything through an instance, whose entity may well register the path;
//! * variables, which are sequential within their process.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use vhdl_syntax::syntax::{NodeKind, SyntaxNode};
use vsg_rs::Parsed;

use crate::design::{all_tokens, assignments, find, is_not_combinational, path, reads, text_of};
use crate::lint::Finding;

/// The signals each signal depends on.
type Graph = BTreeMap<String, BTreeSet<String>>;

/// Add `target <- sources` for every whole-object assignment in a statement.
fn edges(statement: &SyntaxNode, graph: &mut Graph, offsets: &mut BTreeMap<String, usize>) {
    // `q'length` names a property of the signal, not its value, so it is no dependency. The
    // attribute node starts at the tick, so the prefix is the name just before it -- excluded by
    // where it is rather than by name, so the same signal read normally elsewhere still counts.
    let mut sources = Vec::new();
    reads(statement, &mut sources);
    let ticks: Vec<usize> = find(statement, NodeKind::AttributeName)
        .iter()
        .filter_map(|name| {
            all_tokens(name)
                .first()
                .map(vhdl_syntax::syntax::SyntaxToken::text_offset)
        })
        .collect();
    let prefixes: BTreeSet<usize> = ticks
        .iter()
        .filter_map(|tick| {
            sources
                .iter()
                .map(|(_, at)| *at)
                .filter(|at| at < tick)
                .max()
        })
        .collect();
    let sources: BTreeSet<String> = sources
        .into_iter()
        .filter(|(_, at)| !prefixes.contains(at))
        .map(|(name, _)| name)
        .collect();
    for (target, offset) in assignments(statement) {
        // Part of an object: `q(0) <= q(1)` is two different elements, and the whole object
        // would look as though it fed itself.
        if target.contains(['(', '.']) {
            continue;
        }
        // `clk <= not clk after 5 ns` is a delay element, which is how a testbench writes a
        // clock generator; the delay is what breaks the loop.
        if text_of(statement).contains("after") {
            continue;
        }
        let (name, _) = path(&target);
        offsets.entry(name.clone()).or_insert(offset);
        graph
            .entry(name)
            .or_default()
            .extend(sources.iter().cloned());
    }
}

/// The first cycle reachable from `start`, as the signals on it.
fn cycle_from(start: &str, graph: &Graph) -> Option<Vec<String>> {
    let children_of = |name: &str| graph.get(name).cloned().unwrap_or_default().into_iter();
    let mut stack = vec![start.to_owned()];
    let mut on_stack: BTreeSet<String> = stack.iter().cloned().collect();
    let mut visited: BTreeSet<String> = BTreeSet::new();
    // Depth first, keeping the path so the cycle can be named in the finding.
    let mut next = vec![children_of(start)];
    while let Some(children) = next.last_mut() {
        let Some(child) = children.next() else {
            next.pop();
            if let Some(done) = stack.pop() {
                on_stack.remove(&done);
            }
            continue;
        };
        if on_stack.contains(&child) {
            let at = stack.iter().position(|s| *s == child).unwrap_or(0);
            let mut found = stack[at..].to_vec();
            found.push(child);
            return Some(found);
        }
        if !visited.insert(child.clone()) {
            continue;
        }
        next.push(children_of(&child));
        on_stack.insert(child.clone());
        stack.push(child);
    }
    None
}

/// Walk the statements, adding edges for each assignment, and skipping what cannot contribute:
/// a process that is clocked or waits, and an instance (whose entity may register the path). Every other container -- a generate, a block, an `if` -- is descended into, so the
/// edges belong to one assignment rather than to everything around it.
fn collect(node: &SyntaxNode, graph: &mut Graph, offsets: &mut BTreeMap<String, usize>) {
    for child in node.children() {
        match child.kind() {
            // A process that is clocked or that waits is not combinational logic: the register
            // or the wait is what breaks the loop.
            NodeKind::ProcessStatement if is_not_combinational(&child) => {}
            NodeKind::ComponentInstantiationStatement
            | NodeKind::ConcurrentProcedureCallOrComponentInstantiationStatement => {}
            kind if ASSIGNMENTS.contains(&kind) => edges(&child, graph, offsets),
            _ => collect(&child, graph, offsets),
        }
    }
}

/// The assignment kinds that make a combinational edge. A variable assignment is sequential
/// within its process, so it is not one.
const ASSIGNMENTS: &[NodeKind] = &[
    NodeKind::ConcurrentSimpleSignalAssignment,
    NodeKind::ConcurrentConditionalSignalAssignment,
    NodeKind::ConcurrentSelectedSignalAssignment,
    NodeKind::SimpleWaveformAssignment,
    NodeKind::ConditionalWaveformAssignment,
    NodeKind::SelectedWaveformAssignment,
];

pub(crate) fn check(parsed: &Parsed, file: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    for architecture in find(parsed.root(), NodeKind::ArchitectureBody) {
        let mut graph = Graph::new();
        let mut offsets: BTreeMap<String, usize> = BTreeMap::new();
        for part in find(&architecture, NodeKind::ArchitectureStatementPart) {
            collect(&part, &mut graph, &mut offsets);
        }

        let mut reported: BTreeSet<String> = BTreeSet::new();
        for signal in graph.keys() {
            if reported.contains(signal) {
                continue;
            }
            let Some(cycle) = cycle_from(signal, &graph) else {
                continue;
            };
            // One finding per cycle, whichever of its signals is reached first.
            if cycle.iter().any(|s| reported.contains(s)) {
                continue;
            }
            reported.extend(cycle.iter().cloned());
            let Some(offset) = cycle.first().and_then(|s| offsets.get(s)) else {
                continue;
            };
            let (line, column) = parsed.line_col(*offset);
            let through: Vec<&str> = cycle.iter().map(String::as_str).collect();
            // Every signal on the cycle, in order, so a reader can walk it.
            // `cycle` closes on the signal it starts with; that repeat says nothing extra.
            let walk = &cycle[..cycle.len().saturating_sub(1)];
            let related = walk
                .iter()
                .filter_map(|signal| {
                    let at = offsets.get(signal)?;
                    let (line, column) = parsed.line_col(*at);
                    Some(crate::lint::Related {
                        file: file.to_path_buf(),
                        line,
                        column,
                        message: format!("'{signal}' is driven here"),
                    })
                })
                .collect();
            findings.push(Finding {
                file: file.to_path_buf(),
                rule: "lint_720",
                line,
                column,
                message: format!(
                    "Combinational loop: {} depends on itself with no register in the way",
                    through.join(" -> ")
                ),
                related,
            });
        }
    }
    findings.sort_by_key(|f| (f.line, f.column));
    findings
}

/// The rules this module reports, for `--list_rules`.
pub(crate) const RULES: &[(&str, &str)] = &[(
    "lint_720",
    "A signal depends on itself through combinational logic, with no register in the loop.",
)];

#[cfg(test)]
mod tests {
    use super::*;

    fn check_source(source: &str) -> Vec<String> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        check(&parsed, Path::new("dut.vhd"))
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    fn architecture(body: &str) -> String {
        format!(
            "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  \
             signal clk, a, b, y, z : bit;\n\nbegin\n\n{body}\nend architecture rtl;\n"
        )
    }

    #[test]
    fn a_signal_that_feeds_itself_is_a_loop() {
        let found = check_source(&architecture("  y <= y and a;\n"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains('y'), "{found:?}");
    }

    #[test]
    fn a_loop_through_two_signals_is_found() {
        let found = check_source(&architecture("  y <= z and a;\n  z <= y or b;\n"));
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn the_cycle_is_reported_as_places_not_prose() {
        let parsed = Parsed::new(
            architecture("  y <= z and a;\n  z <= y or b;\n")
                .as_bytes()
                .to_vec(),
        );
        let found = check(&parsed, Path::new("dut.vhd"));
        assert_eq!(found.len(), 1, "{found:?}");
        // One location per signal on the cycle, and the closing repeat is not one of them.
        let lines: Vec<usize> = found[0].related.iter().map(|r| r.line).collect();
        assert_eq!(lines.len(), 2, "{:?}", found[0].related);
        assert!(lines.iter().all(|l| *l > 0));
    }

    #[test]
    fn a_register_breaks_the_loop() {
        let found = check_source(&architecture(
            "  p : process (clk) is\n  begin\n    if rising_edge(clk) then\n      \
             y <= z;\n    end if;\n  end process p;\n\n  z <= y and a;\n",
        ));
        assert!(found.is_empty(), "the register breaks it: {found:?}");
    }

    #[test]
    fn a_chain_without_a_cycle_is_quiet() {
        let found = check_source(&architecture("  y <= a and b;\n  z <= y;\n"));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn different_elements_of_one_vector_are_not_a_loop() {
        let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  \
                      signal q : bit_vector(3 downto 0);\n\nbegin\n\n  \
                      q(0) <= q(1);\n  q(1) <= q(2);\n\nend architecture rtl;\n";
        assert!(
            check_source(source).is_empty(),
            "the elements are different signals"
        );
    }

    #[test]
    fn an_attribute_of_the_target_is_not_a_dependency() {
        let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  \
                      signal count : bit_vector(3 downto 0);\n  signal a : bit;\n\nbegin\n\n  \
                      count <= (count'range => a);\n\nend architecture rtl;\n";
        assert!(
            check_source(source).is_empty(),
            "'range is a property, not a value"
        );
    }

    #[test]
    fn a_process_that_waits_is_not_combinational() {
        let found = check_source(&architecture(
            "  p : process is\n  begin\n    wait for 5 ns;\n    clk <= not clk;\n  \
             end process p;\n",
        ));
        assert!(found.is_empty(), "the wait breaks it: {found:?}");
    }

    #[test]
    fn a_delayed_assignment_is_not_a_loop() {
        let found = check_source(&architecture("  clk <= not clk after 5 ns;\n"));
        assert!(found.is_empty(), "the delay breaks it: {found:?}");
    }

    #[test]
    fn a_combinational_process_can_loop() {
        let found = check_source(&architecture(
            "  p : process (a, z) is\n  begin\n    y <= z and a;\n  end process p;\n\n  \
             z <= y;\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
    }
}

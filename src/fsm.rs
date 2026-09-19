//! State machines, read out of the source rather than out of a netlist.
//!
//! A synthesis tool finds an FSM by recognising a register whose type is an enumeration and
//! whose next value is chosen by a `case` on itself. That much is visible in the syntax, so the
//! two checks people buy a linter for — a state nothing can reach, and a state nothing can leave
//! — do not need a netlist:
//!
//! * `lint_710` — a value of the state type that is never assigned. Dead logic at best; at worst
//!   a state that was meant to be reachable and is not.
//! * `lint_711` — a state whose `case` alternative never assigns a different state. The machine
//!   arrives and stays, which is a lock-up unless it is the deliberate end of the design.
//!
//! Both are refused unless the machine can be read with certainty: every assignment to the state
//! has to be a plain value of the type, because a state computed by a function or copied from
//! another signal cannot be reasoned about from the syntax alone. One- and two-process styles
//! both work, since every signal of the state type is considered together.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use vhdl_syntax::syntax::{NodeKind, SyntaxNode, SyntaxToken};
use vsg_rs::Parsed;

use crate::design::{all_tokens, assignments, find, is_clocked, path, text_of};
use crate::lint::Finding;

/// An enumeration type, its values, and the signals declared with it.
struct StateType {
    values: Vec<String>,
    signals: BTreeSet<String>,
}

fn lower(token: &SyntaxToken) -> String {
    String::from_utf8_lossy(token.text().as_bytes()).to_ascii_lowercase()
}

/// Every enumeration type declared in an architecture, with the signals that use it.
fn state_types(architecture: &SyntaxNode) -> BTreeMap<String, StateType> {
    let mut types: BTreeMap<String, StateType> = BTreeMap::new();
    for declaration in find(architecture, NodeKind::FullTypeDeclaration) {
        let Some(enumeration) = find(&declaration, NodeKind::EnumerationTypeDefinition)
            .into_iter()
            .next()
        else {
            continue;
        };
        let Some(name) = all_tokens(&declaration)
            .iter()
            .map(lower)
            .find(|t| t != "type")
        else {
            continue;
        };
        let values: Vec<String> = all_tokens(&enumeration)
            .iter()
            .map(lower)
            .filter(|t| {
                t.chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            })
            .collect();
        // Two values are a flag, not a state machine.
        if values.len() < 3 {
            continue;
        }
        types.insert(
            name,
            StateType {
                values,
                signals: BTreeSet::new(),
            },
        );
    }
    for declaration in find(architecture, NodeKind::SignalDeclaration) {
        let text = text_of(&declaration);
        let Some((names, rest)) = text.split_once(':') else {
            continue;
        };
        let mark = rest
            .trim_end_matches(';')
            .split([':', '('])
            .next()
            .unwrap_or(rest);
        let Some(state) = types.get_mut(mark.trim()) else {
            continue;
        };
        for name in names
            .split(',')
            .map(|n| n.trim().trim_start_matches("signal").trim().to_owned())
            .filter(|n| !n.is_empty())
        {
            state.signals.insert(name);
        }
    }
    types.retain(|_, state| !state.signals.is_empty());
    types
}

/// The values assigned to the state inside a node. `None` for an assignment whose value cannot
/// be read from the syntax, which disqualifies the whole machine.
fn assigned(node: &SyntaxNode, state: &StateType) -> Vec<Option<String>> {
    let mut out = Vec::new();
    for (target, offset) in assignments(node) {
        if !state.signals.contains(&path(&target).0) {
            continue;
        }
        let after: Vec<String> = all_tokens(node)
            .iter()
            .skip_while(|t| t.text_offset() <= offset)
            .map(lower)
            .skip_while(|t| t != "<=" && t != ":=")
            .collect();
        // One value and then the end of the statement: anything else is an expression.
        let value = after.get(1).cloned();
        let alone = after.get(2).is_none_or(|t| t == ";");
        out.push(value.filter(|v| alone && state.values.contains(v)));
    }
    out
}

pub(crate) fn check(parsed: &Parsed, file: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let at = |offset: usize| parsed.line_col(offset);

    for architecture in find(parsed.root(), NodeKind::ArchitectureBody) {
        let processes = find(&architecture, NodeKind::ProcessStatement);
        for (name, state) in state_types(&architecture) {
            // A state machine has a register: some signal of the type is assigned under a clock.
            let clocked = processes.iter().filter(|p| is_clocked(p)).any(|process| {
                assignments(process)
                    .iter()
                    .any(|(target, _)| state.signals.contains(&path(target).0))
            });
            if !clocked {
                continue;
            }
            let values = assigned(&architecture, &state);
            if values.is_empty() || values.iter().any(Option::is_none) {
                continue;
            }
            let reached: BTreeSet<String> = values.into_iter().flatten().collect();

            // lint_710: a value of the type that nothing ever assigns.
            for value in &state.values {
                if reached.contains(value) {
                    continue;
                }
                let Some(offset) = all_tokens(&architecture)
                    .iter()
                    .find(|t| lower(t) == *value)
                    .map(SyntaxToken::text_offset)
                else {
                    continue;
                };
                let (line, column) = at(offset);
                findings.push(Finding {
                    file: file.to_path_buf(),
                    rule: "lint_710",
                    line,
                    column,
                    message: format!(
                        "State '{value}' of '{name}' is never entered: nothing assigns it"
                    ),
                });
            }

            // lint_711: a state whose own alternative never leaves it.
            for case in find(&architecture, NodeKind::CaseStatement) {
                // `case state is`: the selector is the name between the keywords.
                let selector = find(&case, NodeKind::CaseStatementPreamble)
                    .first()
                    .and_then(|preamble| {
                        all_tokens(preamble)
                            .iter()
                            .map(lower)
                            .find(|t| t != "case" && t != "is")
                    })
                    .unwrap_or_default();
                if !state.signals.contains(&selector) {
                    continue;
                }
                // A `case` on the state is only transition logic if it assigns the state
                // somewhere. One that does not is a multiplexer choosing an output by state,
                // and has no exits to be missing.
                if assigned(&case, &state).is_empty() {
                    continue;
                }
                for alternative in find(&case, NodeKind::CaseStatementAlternative) {
                    let Some(preamble) =
                        find(&alternative, NodeKind::CaseStatementAlternativePreamble)
                            .into_iter()
                            .next()
                    else {
                        continue;
                    };
                    // `when others` and multi-value choices cover more than one state.
                    let choices: Vec<String> = all_tokens(&preamble)
                        .iter()
                        .map(lower)
                        .filter(|t| state.values.contains(t))
                        .collect();
                    if choices.len() != 1 || text_of(&preamble).contains("others") {
                        continue;
                    }
                    let here = &choices[0];
                    let leaves = assigned(&alternative, &state)
                        .into_iter()
                        .flatten()
                        .any(|value| value != *here);
                    if leaves {
                        continue;
                    }
                    let (line, column) = at(all_tokens(&preamble)
                        .first()
                        .map_or(0, SyntaxToken::text_offset));
                    findings.push(Finding {
                        file: file.to_path_buf(),
                        rule: "lint_711",
                        line,
                        column,
                        message: format!(
                            "State '{here}' of '{name}' has no exit: its alternative never \
                             assigns another state"
                        ),
                    });
                }
            }
        }
    }
    findings.sort_by_key(|f| (f.line, f.column, f.rule));
    findings
}

/// The rules this module reports, for `--list_rules`.
pub(crate) const RULES: &[(&str, &str)] = &[
    (
        "lint_710",
        "A state of an enumerated state machine is never entered.",
    ),
    (
        "lint_711",
        "A state of an enumerated state machine has no exit.",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn check_source(source: &str) -> Vec<(&'static str, String)> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        check(&parsed, Path::new("dut.vhd"))
            .into_iter()
            .map(|f| (f.rule, f.message))
            .collect()
    }

    /// A machine over `idle`, `run`, `done` and `orphan`, with the alternatives the test gives.
    fn machine(body: &str) -> String {
        format!(
            "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  \
             type state_t is (idle, run, done, orphan);\n\n  signal state : state_t;\n  \
             signal clk, go : bit;\n\nbegin\n\n  p : process (clk) is\n  begin\n    \
             if rising_edge(clk) then\n      case state is\n{body}      end case;\n    \
             end if;\n  end process p;\n\nend architecture rtl;\n"
        )
    }

    #[test]
    fn a_state_nothing_assigns_is_unreachable() {
        let found = check_source(&machine(
            "        when idle =>\n          if go = '1' then\n            state <= run;\n          \
             end if;\n        when run =>\n          state <= done;\n        when done =>\n          \
             state <= idle;\n        when orphan =>\n          state <= idle;\n",
        ));
        let unreachable: Vec<&String> = found
            .iter()
            .filter(|(rule, _)| *rule == "lint_710")
            .map(|(_, message)| message)
            .collect();
        assert_eq!(unreachable.len(), 1, "{found:?}");
        assert!(unreachable[0].contains("'orphan'"), "{unreachable:?}");
    }

    #[test]
    fn a_state_that_never_leaves_is_reported() {
        let found = check_source(&machine(
            "        when idle =>\n          state <= run;\n        when run =>\n          \
             state <= done;\n        when done =>\n          null;\n        when orphan =>\n          \
             state <= idle;\n",
        ));
        let stuck: Vec<&String> = found
            .iter()
            .filter(|(rule, _)| *rule == "lint_711")
            .map(|(_, message)| message)
            .collect();
        assert_eq!(stuck.len(), 1, "{found:?}");
        assert!(stuck[0].contains("'done'"), "{stuck:?}");
    }

    #[test]
    fn a_complete_machine_is_quiet() {
        let found = check_source(&machine(
            "        when idle =>\n          if go = '1' then\n            state <= run;\n          \
             end if;\n        when run =>\n          state <= done;\n        when done =>\n          \
             state <= orphan;\n        when orphan =>\n          state <= idle;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_state_computed_rather_than_named_is_not_guessed_at() {
        // `state <= next_state(state)` cannot be read from the syntax, so the machine is left
        // alone rather than half-understood.
        let found = check_source(&machine(
            "        when idle =>\n          state <= run;\n        when run =>\n          \
             state <= done;\n        when done =>\n          state <= next_state(state);\n        \
             when orphan =>\n          state <= idle;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_case_that_only_chooses_an_output_is_not_transition_logic() {
        // `case state is when a => y <= ...` is a multiplexer: it selects by state rather than
        // deciding the next one, so no alternative is missing an exit.
        let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  \
                      type state_t is (idle, run, done);\n\n  signal state : state_t;\n  \
                      signal clk, go, y : bit;\n\nbegin\n\n  p : process (clk) is\n  begin\n    \
                      if rising_edge(clk) then\n      if go = '1' then\n        state <= run;\n      \
                      else\n        state <= idle;\n      end if;\n      if go = '0' then\n        \
                      state <= done;\n      end if;\n      case state is\n        when idle =>\n          \
                      y <= '0';\n        when run =>\n          y <= '1';\n        when done =>\n          \
                      y <= '0';\n      end case;\n    end if;\n  end process p;\n\n\
                      end architecture rtl;\n";
        let found = check_source(source);
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_711"),
            "a multiplexer, not a machine: {found:?}"
        );
    }

    #[test]
    fn a_process_without_a_clock_is_not_a_state_machine() {
        let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  \
                      type state_t is (idle, run, orphan);\n\n  signal state : state_t;\n  \
                      signal go : bit;\n\nbegin\n\n  p : process (go) is\n  begin\n    \
                      case state is\n      when idle =>\n        state <= run;\n      \
                      when others =>\n        state <= idle;\n    end case;\n  \
                      end process p;\n\nend architecture rtl;\n";
        assert!(check_source(source).is_empty(), "no register, no machine");
    }
}

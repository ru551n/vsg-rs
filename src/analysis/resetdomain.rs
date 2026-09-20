//! Reset domains, and registers that cross between them.
//!
//! A register belongs to the reset that clears it. When a register reset by one signal feeds a
//! register reset by another, the two can be released at different moments: the source stops
//! being held while the destination is still in reset, or the other way round. The destination
//! then captures a value from a register that is mid-release, and an asynchronous release near
//! the clock edge is a recovery or removal violation, which is metastability by another name.
//!
//! It is the same shape as a clock domain crossing, and the same shape of mistake, but the
//! domains are drawn by resets rather than clocks. `lint_700` finds one, `lint_701` the other.
//!
//! Like `lint_700`, this is **experimental and off by default**, for the same reason: the rule
//! has to decide which signal is a reset before it can decide anything else, and the source does
//! not say. A reset is recognised only where it is written the way a reset is written, and
//! everything else is left alone.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::Parsed;
use vhdl_syntax::syntax::{NodeKind, SyntaxNode};

use super::design::{all_tokens, assignments, find, is_clocked, lower, path, reads};
use super::lint::Finding;

/// The reset a clocked process clears its registers with, if it has one.
///
/// One shape counts:
///
/// ```vhdl
/// if rst = '1' then           -- the branch the clock edge is the `elsif` of
///   q <= '0';
/// elsif rising_edge(clk) then
/// ```
///
/// An asynchronous reset, in a process that is edge triggered. A reset written any other way is
/// not recognised, which is the intended failure: under-reporting costs a missed crossing, and
/// guessing costs a false accusation about logic that is doing its job.
fn reset_of(process: &SyntaxNode) -> Option<String> {
    if !is_clocked(process) {
        return None;
    }
    let words: Vec<String> = all_tokens(process).iter().map(lower).collect();
    let edge = |at: usize| {
        words
            .get(at)
            .is_some_and(|t| t == "rising_edge" || t == "falling_edge" || t == "'event")
    };
    // The process's own statements start after its `begin`.
    let body = words.iter().position(|t| t == "begin")? + 1;
    // `if <name> = '<bit>' then`, read at one exact place and nowhere else. A reset only ever
    // appears as the first thing a clocked process tests; `if in_ready_i = '1' then` further in
    // is a handshake, and reading it as a reset put seven false findings on open-logic.
    let guard = |at: usize| -> Option<String> {
        let name = words.get(at + 1)?;
        if words.get(at)? != "if"
            || words.get(at + 2)? != "="
            || !matches!(words.get(at + 3)?.as_str(), "'1'" | "'0'")
            || words.get(at + 4)? != "then"
        {
            return None;
        }
        Some(name.clone())
    };

    // Asynchronous: the branch before the clock edge, which the edge is the `elsif` of.
    if let Some(name) = guard(body)
        && words
            .iter()
            .enumerate()
            .any(|(at, t)| t == "elsif" && edge(at + 1))
    {
        return Some(name);
    }
    // A synchronous reset is not read, and not only out of caution. The hazard this rule is
    // about is a reset asserting or releasing *asynchronously*, away from the clock edge; a
    // reset sampled by the clock is ordinary clocked logic and crosses nothing. Reading the
    // first test inside `if rising_edge(clk) then` also cannot tell a reset from a clock
    // enable, which put five false findings on cnn_accel: `add_pipe_en` is not a reset.
    None
}

/// Whether a statement only captures one signal into another: `flop <= source;` and nothing
/// else. Taking a crossing into a register of the destination's own domain before using it is
/// how the crossing is made safe, exactly as for clocks.
fn is_plain_capture(statement: &SyntaxNode, source: &str) -> bool {
    let words: Vec<String> = all_tokens(statement)
        .iter()
        .map(lower)
        .filter(|t| t != ";")
        .collect();
    words.len() == 3 && words[1] == "<=" && words[2] == source
}

/// The assignment kinds inside a process that can read another domain.
const ASSIGNMENTS: &[NodeKind] = &[
    NodeKind::SimpleWaveformAssignment,
    NodeKind::ConditionalWaveformAssignment,
    NodeKind::SelectedWaveformAssignment,
];

/// Report registers read across a reset domain boundary.
#[must_use]
pub fn check(parsed: &Parsed, file: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    for architecture in find(parsed.root(), NodeKind::ArchitectureBody) {
        let processes = find(&architecture, NodeKind::ProcessStatement);

        // Which reset each register belongs to. A register cleared by two different resets is
        // not a crossing anyone can reason about, so it is dropped rather than guessed at.
        let mut domain: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for process in &processes {
            let Some(reset) = reset_of(process) else {
                continue;
            };
            for (target, _) in assignments(process) {
                domain
                    .entry(path(&target).0)
                    .or_default()
                    .insert(reset.clone());
            }
        }
        domain.retain(|_, resets| resets.len() == 1);
        if domain.values().flatten().collect::<BTreeSet<_>>().len() < 2 {
            // One reset: nothing can cross.
            continue;
        }

        for process in &processes {
            let Some(reset) = reset_of(process) else {
                continue;
            };
            for kind in ASSIGNMENTS {
                for statement in find(process, *kind) {
                    let mut named = Vec::new();
                    reads(&statement, &mut named);
                    for (name, offset) in named {
                        // A reset crossing domains is what a reset does.
                        if name == reset {
                            continue;
                        }
                        let Some(other) = domain
                            .get(&name)
                            .and_then(|resets| resets.iter().next())
                            .filter(|other| **other != reset)
                        else {
                            continue;
                        };
                        if is_plain_capture(&statement, &name) {
                            continue;
                        }
                        let (line, column) = parsed.line_col(offset);
                        findings.push(Finding {
                            file: file.to_path_buf(),
                            rule: "lint_701",
                            line,
                            column,
                            message: format!(
                                "Signal '{name}' is reset by '{other}' and used in logic reset by \
                                 '{reset}': the two can be released at different moments, so this \
                                 reads a register that is still coming out of reset"
                            ),
                            related: Vec::new(),
                        });
                    }
                }
            }
        }
    }
    incomplete_resets(parsed, file, &mut findings);
    findings.sort_by_key(|f| (f.line, f.column));
    findings.dedup_by(|a, b| a.line == b.line && a.message == b.message);
    findings
}

/// Report registers a clocked process assigns but its reset branch leaves alone.
///
/// The reset branch says what the design starts from. A register assigned in the clocked branch
/// and not in the reset branch starts from whatever it happens to hold, which is usually an
/// oversight and sometimes deliberate: a data path register often costs more to reset than the
/// reset is worth, and designers leave those out on purpose. The rule states the fact and leaves
/// the judgement, which is why it is advisory.
fn incomplete_resets(parsed: &Parsed, file: &Path, findings: &mut Vec<Finding>) {
    for process in find(parsed.root(), NodeKind::ProcessStatement) {
        let Some(reset) = reset_of(&process) else {
            continue;
        };
        // Everything before the `elsif` that carries the clock edge is the reset branch.
        let tokens = all_tokens(&process);
        let Some(boundary) = tokens.iter().enumerate().find_map(|(at, token)| {
            let next = tokens.get(at + 1).map(lower)?;
            (lower(token) == "elsif"
                && matches!(next.as_str(), "rising_edge" | "falling_edge" | "'event"))
            .then(|| token.text_offset())
        }) else {
            continue;
        };

        let mut cleared: BTreeSet<String> = BTreeSet::new();
        let mut assigned: BTreeMap<String, usize> = BTreeMap::new();
        for (target, offset) in assignments(&process) {
            let name = path(&target).0;
            if offset < boundary {
                cleared.insert(name);
            } else {
                assigned.entry(name).or_insert(offset);
            }
        }
        for (name, offset) in assigned {
            if cleared.contains(&name) {
                continue;
            }
            let (line, column) = parsed.line_col(offset);
            findings.push(Finding {
                file: file.to_path_buf(),
                rule: "lint_702",
                line,
                column,
                message: format!(
                    "Signal \'{name}\' is assigned by this process but not by its \'{reset}\' \
                     branch, so it starts from whatever it happens to hold rather than from a \
                     known value"
                ),
                related: Vec::new(),
            });
        }
    }
}

/// The rules this module reports, for `--list_rules`.
pub const RULES: &[super::Rule] = &[
    super::Rule {
        id: "lint_701",
        description: "A register reset by one signal is used in logic reset by another.",
        certainty: super::Certainty::Experimental,
    },
    super::Rule {
        id: "lint_702",
        description: "A register a clocked process assigns but its reset branch does not.",
        certainty: super::Certainty::Advisory,
    },
];

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

    /// Two clocked processes, `p_a` reset by `rst_a` and `p_b` by `rst_b`, with `body_b` as the
    /// clocked part of the second.
    fn two_domains(body_b: &str) -> String {
        format!(
            "entity dut is\n  port (\n    clk   : in bit;\n    rst_a : in bit;\n    \
             rst_b : in bit;\n    d     : in bit\n  );\nend entity dut;\n\n\
             architecture rtl of dut is\n  signal in_a, in_b, out_b : bit;\nbegin\n\n  \
             p_a : process (clk, rst_a) is\n  begin\n    if rst_a = '1' then\n      \
             in_a <= '0';\n    elsif rising_edge(clk) then\n      in_a <= d;\n    end if;\n  \
             end process p_a;\n\n  p_b : process (clk, rst_b) is\n  begin\n    \
             if rst_b = '1' then\n      in_b <= '0';\n      out_b <= '0';\n    \
             elsif rising_edge(clk) then\n{body_b}    end if;\n  end process p_b;\n\n\
             end architecture rtl;\n"
        )
    }

    #[test]
    fn a_register_from_another_reset_domain_used_in_logic() {
        let found = check_source(&two_domains("      out_b <= in_a and d;\n"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("'in_a' is reset by 'rst_a'"), "{found:?}");
        assert!(found[0].contains("reset by 'rst_b'"), "{found:?}");
    }

    #[test]
    fn capturing_it_first_is_how_the_crossing_is_made_safe() {
        // `in_b <= in_a;` and nothing else: the value is taken into the destination's own
        // domain before it is used, which is a reset synchroniser's first stage.
        let found = check_source(&two_domains("      in_b <= in_a;\n"));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn staying_inside_one_reset_domain_is_silent() {
        let found = check_source(&two_domains("      out_b <= in_b and d;\n"));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn one_reset_cannot_cross_anything() {
        let source = "entity dut is\n  port (clk : in bit; rst : in bit; d : in bit);\n\
                      end entity dut;\n\narchitecture rtl of dut is\n  signal a, b : bit;\n\
                      begin\n  p : process (clk, rst) is\n  begin\n    if rst = '1' then\n      \
                      a <= '0';\n      b <= '0';\n    elsif rising_edge(clk) then\n      \
                      a <= d;\n      b <= a and d;\n    end if;\n  end process p;\n\
                      end architecture rtl;\n";
        assert!(check_source(source).is_empty());
    }

    #[test]
    fn a_process_with_no_reset_is_not_a_domain() {
        let source = "entity dut is\n  port (clk : in bit; d : in bit);\nend entity dut;\n\n\
                      architecture rtl of dut is\n  signal a, b : bit;\nbegin\n  \
                      p : process (clk) is\n  begin\n    if rising_edge(clk) then\n      \
                      a <= d;\n      b <= a and d;\n    end if;\n  end process p;\n\
                      end architecture rtl;\n";
        assert!(check_source(source).is_empty());
    }

    #[test]
    fn a_combinational_process_has_no_reset_domain() {
        // `if en = '1' then` is not a reset merely because it guards an assignment: with no
        // clock edge there is no register here to reset.
        let source = "entity dut is\n  port (en : in bit; d : in bit);\nend entity dut;\n\n\
                      architecture rtl of dut is\n  signal a : bit;\nbegin\n  \
                      p : process (en, d) is\n  begin\n    a <= '0';\n    if en = '1' then\n      \
                      a <= d;\n    end if;\n  end process p;\nend architecture rtl;\n";
        assert!(check_source(source).is_empty());
    }
}

#[cfg(test)]
mod reset_branch_tests {
    use super::*;

    fn check_source(source: &str) -> Vec<String> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        check(&parsed, Path::new("dut.vhd"))
            .into_iter()
            .filter(|f| f.rule == "lint_702")
            .map(|f| f.message)
            .collect()
    }

    /// One clocked process with an asynchronous reset: `cleared` in the reset branch, `clocked`
    /// after it.
    fn process(cleared: &str, clocked: &str) -> String {
        format!(
            "entity dut is\n  port (clk : in bit; rst : in bit; d : in bit);\nend entity dut;\n\n\
             architecture rtl of dut is\n  signal a, b : bit;\nbegin\n  \
             p : process (clk, rst) is\n  begin\n    if rst = '1' then\n{cleared}    \
             elsif rising_edge(clk) then\n{clocked}    end if;\n  end process p;\n\
             end architecture rtl;\n"
        )
    }

    #[test]
    fn a_register_the_reset_branch_forgets() {
        let found = check_source(&process(
            "      a <= '0';\n",
            "      a <= d;\n      b <= d;\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("'b' is assigned"), "{found:?}");
        assert!(found[0].contains("'rst' branch"), "{found:?}");
    }

    #[test]
    fn a_reset_branch_that_covers_everything_is_silent() {
        let found = check_source(&process(
            "      a <= '0';\n      b <= '0';\n",
            "      a <= d;\n      b <= d;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_process_with_no_reset_has_no_branch_to_judge() {
        let source = "entity dut is\n  port (clk : in bit; d : in bit);\nend entity dut;\n\n\
                      architecture rtl of dut is\n  signal a : bit;\nbegin\n  \
                      p : process (clk) is\n  begin\n    if rising_edge(clk) then\n      \
                      a <= d;\n    end if;\n  end process p;\nend architecture rtl;\n";
        assert!(check_source(source).is_empty());
    }
}

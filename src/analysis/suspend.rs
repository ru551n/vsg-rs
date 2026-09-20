//! Processes that can never suspend.
//!
//! A process repeats for ever. What stops it monopolising the simulation is that it suspends:
//! at a `wait`, or at the end of its body when it has a sensitivity list, which is the same
//! thing written differently. A process with neither reaches the end of its body, starts again,
//! and never yields -- so time never advances and the simulation makes no progress. It is not a
//! design that behaves oddly; it is a design that cannot run.
//!
//! Both simulators agree, and both say so at analysis: GHDL reports "infinite loop for this
//! process without a wait statement" and NVC "potential infinite loop in process with no
//! sensitivity list and no wait statements".
//!
//! NVC's *potential* is the whole difficulty. A process whose body calls a procedure may suspend
//! inside that procedure, because a procedure -- unlike a function, which the LRM forbids to
//! contain a `wait` -- is allowed to. Proving otherwise means resolving the call, finding the
//! body, and doing the same for everything it calls. This module does not attempt that: a
//! process that calls any procedure is left alone. It under-reports, and every finding it makes
//! is one where no call can be hiding the `wait`.

use std::path::Path;

use crate::Parsed;
use vhdl_syntax::syntax::{NodeKind, SyntaxNode};

use super::design::{all_tokens, find, text_of};
use super::lint::Finding;

/// ` 'p'`, or nothing when the process has no label to name it by.
fn label_of(process: &SyntaxNode) -> String {
    // `p : process` -- the label is the statement's first token, and the colon after it is what
    // distinguishes it from the `process` keyword of an unlabelled one.
    let tokens = all_tokens(process);
    let (Some(first), Some(second)) = (tokens.first(), tokens.get(1)) else {
        return String::new();
    };
    if text_of_token(second) != ":" {
        return String::new();
    }
    format!(" '{}'", text_of_token(first).to_ascii_lowercase())
}

/// One token's text.
fn text_of_token(token: &vhdl_syntax::syntax::SyntaxToken) -> String {
    token.text().to_string()
}

/// Report every process that cannot suspend.
#[must_use]
pub fn check(parsed: &Parsed, file: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    for process in find(parsed.root(), NodeKind::ProcessStatement) {
        // A sensitivity list is a `wait` at the end of the body, spelled differently. Any form
        // of one counts, including VHDL-2008's `process (all)`, which names no signals and so
        // produces no list of them -- reading that as "no sensitivity list" reported 33 sound
        // processes in open-logic, which writes every combinational process that way.
        let preamble = find(&process, NodeKind::ProcessPreamble);
        let sensitivity = !find(&process, NodeKind::SensitivityList).is_empty()
            || !find(&process, NodeKind::SensitivityClause).is_empty()
            || preamble
                .first()
                .is_some_and(|node| text_of(node).contains('('));
        if sensitivity {
            continue;
        }
        // Any `wait` at all, reachable or not. Deciding reachability is a second proof this
        // rule does not need, and getting it wrong would mean reporting a process that does
        // suspend.
        if !find(&process, NodeKind::WaitStatement).is_empty() {
            continue;
        }
        // A procedure may hold the `wait` this process is missing. Resolving the call, and
        // every call below it, is the only way to know -- which is why NVC says "potential".
        if !find(&process, NodeKind::ProcedureCallStatement).is_empty() {
            continue;
        }

        let Some(first) = all_tokens(&process).first().cloned() else {
            continue;
        };
        let (line, column) = parsed.line_col(first.text_offset());
        let named = label_of(&process);
        findings.push(Finding {
            file: file.to_path_buf(),
            rule: "lint_770",
            line,
            column,
            message: format!(
                "Process{named} can never suspend: it has no sensitivity list and no wait \
                 statement, so it restarts without ever letting simulation time advance."
            ),
            related: Vec::new(),
        });
    }
    findings.sort_by_key(|f| (f.line, f.column));
    findings
}

/// The rules this module reports, for `--list_rules`.
pub const RULES: &[super::Rule] = &[super::Rule {
    id: "lint_770",
    description: "A process with no sensitivity list and no wait statement, which can never \
                  suspend.",
    certainty: super::Certainty::Definite,
}];

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

    /// An architecture whose statement part is `body`.
    fn architecture(body: &str) -> String {
        format!(
            "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n  signal x : bit;\n\
             begin\n{body}end architecture rtl;\n"
        )
    }

    #[test]
    fn a_process_with_neither_a_list_nor_a_wait() {
        let found = check_source(&architecture(
            "  p : process\n  begin\n    x <= '1';\n  end process p;\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("'p' can never suspend"), "{found:?}");
    }

    #[test]
    fn process_all_is_a_sensitivity_list_too() {
        // VHDL-2008's implicit list. open-logic writes every combinational process this way,
        // and reading it as "no sensitivity list" reported 33 perfectly good ones.
        let found = check_source(&architecture(
            "  p : process (all) is\n  begin\n    x <= '1';\n  end process p;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_sensitivity_list_is_a_wait_at_the_end() {
        let found = check_source(&architecture(
            "  p : process (x) is\n  begin\n    x <= '1';\n  end process p;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_wait_suspends_it() {
        let found = check_source(&architecture(
            "  p : process\n  begin\n    x <= '1';\n    wait for 10 ns;\n  end process p;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_wait_that_looks_unreachable_still_counts() {
        let found = check_source(&architecture(
            "  p : process\n  begin\n    if false then\n      wait;\n    end if;\n  \
             end process p;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_procedure_call_might_be_where_the_wait_is() {
        // The reason NVC says "potential": the procedure may suspend, so this process may be
        // perfectly well behaved. Saying nothing is the only honest answer without resolving
        // the call.
        let found = check_source(&architecture(
            "  p : process\n  begin\n    tick;\n  end process p;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_process_without_a_label_is_still_reported() {
        let found = check_source(&architecture(
            "  process\n  begin\n    x <= '1';\n  end process;\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("Process can never suspend"),
            "{found:?}"
        );
    }
}

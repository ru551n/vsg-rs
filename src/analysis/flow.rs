//! Control flow inside a subprogram: a path that returns nothing, and statements nothing reaches.
//!
//! Both rules are about the shape of the statements rather than what they compute, so both are
//! decidable from the source. Neither compiler catches either one. GHDL and NVC analyse
//!
//! ```vhdl
//! function f (x : integer) return integer is
//! begin
//!     if x > 0 then
//!         return 1;
//!     end if;
//! end function;
//! ```
//!
//! without a word. GHDL says `missing return in function` at **run time**, and only if the run
//! reaches the call with a value that takes the silent path; NVC says nothing at all. That is
//! the argument for this layer in one example: the answer was in the source the whole time.
//!
//! # What "every path" means here
//!
//! Uncertainty resolves towards silence, and the direction differs per rule. Asking whether a
//! function *can* fall through, a loop is assumed to return, because a loop this module cannot
//! read must not become an accusation. Asking whether a statement is unreachable, the same loop
//! is assumed to finish, because what follows it is then unreachable only if something surer
//! than a loop says so.

use std::path::Path;

use crate::Parsed;
use vhdl_syntax::syntax::{NodeKind, SyntaxNode};

use super::design::{all_tokens, body, find, lower};
use super::lint::Finding;

/// Whether control always leaves the statement list at this statement.
///
/// `lenient` is for the caller asking whether a function *can* fall through, where anything
/// unclear must count as an ending so that correct code is not accused. The caller asking what
/// is unreachable passes `false`, and then only a statement that certainly transfers counts.
fn transfers(statement: &SyntaxNode, lenient: bool) -> bool {
    let words = || {
        all_tokens(statement)
            .iter()
            .map(lower)
            .collect::<Vec<String>>()
    };
    match statement.kind() {
        NodeKind::ReturnStatement => true,
        // `exit when c;` leaves only sometimes. The condition is not evaluated here, so a
        // conditional exit transfers nothing this module can rely on.
        NodeKind::ExitStatement | NodeKind::NextStatement => !words().iter().any(|w| w == "when"),
        // `assert false` is how an author writes "this cannot happen", so a function ending in
        // one is not falling through by accident, whatever severity it carries. A rule in the
        // definite class does not argue with intent stated that plainly.
        //
        // It is not a transfer of control though: an assertion reports and execution carries
        // on, which is why the unreachable-statement rule does not accept one. open-logic has
        // defensive assertions followed by ordinary statements, and calling those dead was
        // simply wrong.
        NodeKind::AssertionStatement | NodeKind::ReportStatement => {
            let words = words();
            lenient
                && (words.iter().any(|w| w == "false")
                    || (words.iter().any(|w| w == "severity")
                        && words.iter().any(|w| w == "failure")))
        }
        NodeKind::IfStatement => {
            let Some(otherwise) = statement
                .children()
                .find(|c| c.kind() == NodeKind::IfStatementElse)
            else {
                // No `else`, so one path falls out of the `if` having done nothing.
                return false;
            };
            let branches: Vec<Vec<SyntaxNode>> = std::iter::once(body(statement))
                .chain(
                    statement
                        .children()
                        .filter(|c| c.kind() == NodeKind::IfStatementElsif)
                        .map(|elsif| body(&elsif)),
                )
                .chain(std::iter::once(body(&otherwise)))
                .collect();
            branches.iter().all(|b| transfers_list(b, lenient))
        }
        NodeKind::CaseStatement => {
            let alternatives: Vec<SyntaxNode> = statement
                .children()
                .filter(|c| c.kind() == NodeKind::CaseStatementAlternative)
                .collect();
            // No `when others` is needed. The LRM requires a case statement's choices to cover
            // every value of its subtype, so a case that analyses at all is exhaustive, and an
            // incomplete one is reported by the front end as its own finding. VUnit's
            // `case p_axi_slave_type is when read_slave => return ...; when write_slave =>
            // return ...; end case;` returns on every path and has no `others` to say so.
            if alternatives.is_empty() {
                return false;
            }
            alternatives
                .iter()
                .all(|a| transfers_list(&body(a), lenient))
        }
        NodeKind::LoopStatement => lenient,
        _ => false,
    }
}

/// Whether control always leaves this list of statements before running off its end.
fn transfers_list(statements: &[SyntaxNode], lenient: bool) -> bool {
    statements.iter().any(|s| transfers(s, lenient))
}

/// The name a function specification declares, and where it is.
fn designator(specification: &SyntaxNode) -> Option<(String, usize)> {
    all_tokens(specification)
        .iter()
        .find(|t| !matches!(lower(t).as_str(), "function" | "impure" | "pure"))
        .map(|t| (t.text().to_string(), t.text_range().start))
}

/// `lint_771`: a function body that can run off its end without returning a value.
fn missing_return(parsed: &Parsed, file: &Path, findings: &mut Vec<Finding>) {
    for subprogram in find(parsed.root(), NodeKind::SubprogramBody) {
        // The body's own specification, from its preamble. Not a recursive search: a procedure
        // that declares a helper function would find the helper's and be judged as though it
        // were that function, which is 72 findings in one file of correct code.
        let Some(specification) = subprogram
            .children()
            .filter(|c| c.kind() == NodeKind::SubprogramBodyPreamble)
            .flat_map(|preamble| preamble.children().collect::<Vec<SyntaxNode>>())
            .find(|c| c.kind() == NodeKind::FunctionSpecification)
        else {
            continue;
        };
        // The declarative part holds nested subprograms, which are bodies in their own right and
        // are reached by this same walk. Only the statements after `begin` are this one's.
        let statements: Vec<SyntaxNode> = subprogram
            .children()
            .filter(|c| c.kind() == NodeKind::SubprogramStatementPart)
            .flat_map(|part| part.children().collect::<Vec<SyntaxNode>>())
            .collect();
        if transfers_list(&statements, true) {
            continue;
        }
        let Some((name, offset)) = designator(&specification) else {
            continue;
        };
        let (line, column) = parsed.line_col(offset);
        findings.push(Finding {
            file: file.to_path_buf(),
            rule: "lint_771",
            line,
            column,
            message: format!(
                "'{name}' can reach the end of its body without returning a value, \
                 which is an error when it does"
            ),
            related: Vec::new(),
        });
    }
}

/// `lint_772`: a statement after one that always transfers control away.
fn unreachable(parsed: &Parsed, file: &Path, findings: &mut Vec<Finding>) {
    // A list of sequential statements is a `SequenceOfStatements` inside a branch, and a
    // `...StatementPart` where a process or a subprogram holds its own.
    let lists = find(parsed.root(), NodeKind::SequenceOfStatements)
        .into_iter()
        .chain(find(parsed.root(), NodeKind::ProcessStatementPart))
        .chain(find(parsed.root(), NodeKind::SubprogramStatementPart));
    for sequence in lists {
        let statements: Vec<SyntaxNode> = sequence.children().collect();
        for (at, statement) in statements.iter().enumerate() {
            if !transfers(statement, false) {
                continue;
            }
            // Only the first statement after it. The rest are unreachable for the same reason,
            // and one finding per dead branch is the useful number.
            let Some(next) = statements.get(at + 1) else {
                continue;
            };
            // `-- synthesis translate_off` around a `return` makes what follows unreachable in
            // simulation and reachable in synthesis, which is exactly how a design asks which
            // of the two it is in. The text between the two statements is the only place the
            // pragma that closes such a region can be.
            let gap = statement.text_range().end..next.text_range().start;
            let between = String::from_utf8_lossy(&parsed.source()[gap]).to_ascii_lowercase();
            if between.contains("translate_o") {
                continue;
            }
            let keyword = all_tokens(statement)
                .first()
                .map_or_else(|| "return".to_owned(), lower);
            let offset: usize = next.text_range().start;
            let (line, column) = parsed.line_col(offset);
            findings.push(Finding {
                file: file.to_path_buf(),
                rule: "lint_772",
                line,
                column,
                message: format!(
                    "Nothing reaches this statement: the `{keyword}` above it always \
                     transfers control"
                ),
                related: Vec::new(),
            });
        }
    }
}

pub fn check(parsed: &Parsed, file: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    missing_return(parsed, file, &mut findings);
    unreachable(parsed, file, &mut findings);
    findings.sort_by_key(|f| (f.line, f.column));
    findings
}

/// The rules this module reports, for `--list_rules`.
pub const RULES: &[super::Rule] = &[
    super::Rule {
        id: "lint_771",
        description: "A function that can reach the end of its body without returning a value.",
        certainty: super::Certainty::Definite,
    },
    super::Rule {
        id: "lint_772",
        description: "A statement that nothing can reach.",
        certainty: super::Certainty::Definite,
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
            .map(|f| format!("{} {}", f.rule, f.message))
            .collect()
    }

    fn wrap(body: &str) -> String {
        format!(
            "package p is\n  function f (x : integer) return integer;\nend package;\n\
             package body p is\n  function f (x : integer) return integer is\n  begin\n\
             {body}\n  end function;\nend package body;\n"
        )
    }

    #[test]
    fn a_function_that_can_fall_through_is_reported() {
        let found = check_source(&wrap("    if x > 0 then\n      return 1;\n    end if;"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("lint_771 'f' can reach"), "{found:?}");
    }

    #[test]
    fn a_return_on_every_branch_is_silent() {
        let found = check_source(&wrap(
            "    if x > 0 then\n      return 1;\n    else\n      return 0;\n    end if;",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_case_that_returns_everywhere_is_silent() {
        let found = check_source(&wrap(
            "    case x is\n      when 0 => return 0;\n      when others => return 1;\n    \
             end case;",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    /// The idiom for a path the author says cannot happen.
    #[test]
    fn an_assertion_that_always_fails_is_an_ending() {
        let found = check_source(&wrap(
            "    if x > 0 then\n      return 1;\n    end if;\n    \
             assert false report \"unreachable\" severity failure;",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    /// A loop this module cannot read must not become an accusation.
    #[test]
    fn a_loop_is_assumed_to_return() {
        let found = check_source(&wrap(
            "    for i in 0 to 3 loop\n      return i;\n    end loop;",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_statement_after_a_return_is_unreachable() {
        let found = check_source(&wrap("    return x;\n    return x + 1;"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("lint_772 Nothing reaches"),
            "{found:?}"
        );
    }

    /// `VUnit`'s `case p_axi_slave_type is when read_slave => ... when write_slave => ...`.
    /// The LRM requires a case to cover its subtype, so no `others` is needed to know it does.
    #[test]
    fn a_case_without_others_still_covers_its_type() {
        let found = check_source(&wrap(
            "    case x is\n      when 0 => return 0;\n      when 1 => return 1;\n    \
             end case;",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    /// A procedure that declares a helper function is not that function.
    #[test]
    fn a_procedure_holding_a_function_is_not_judged_as_one() {
        let source = "\
package body p is
  procedure run (x : in integer) is
    function helper return string is
    begin
      return \"\";
    end function;
  begin
    report helper;
  end procedure;
end package body;
";
        assert!(
            check_source(source).is_empty(),
            "{:?}",
            check_source(source)
        );
    }

    /// An assertion reports and carries on, so what follows it is reached.
    #[test]
    fn a_statement_after_an_assertion_is_reachable() {
        let source = "\
package body p is
  procedure run (x : in integer) is
  begin
    assert x > 0 report \"bad\" severity failure;
    report \"still here\";
  end procedure;
end package body;
";
        assert!(
            check_source(source).is_empty(),
            "{:?}",
            check_source(source)
        );
    }

    /// The idiom for asking which tool is reading the file: unreachable in simulation on
    /// purpose, and the only statement the synthesis tool keeps.
    #[test]
    fn a_synthesis_pragma_makes_what_follows_reachable() {
        let source = "\
package body p is
  function in_simulation return boolean is
  begin
    -- synthesis translate_off
    return true;
    -- synthesis translate_on

    return false;
  end function;
end package body;
";
        assert!(
            check_source(source).is_empty(),
            "{:?}",
            check_source(source)
        );
    }

    #[test]
    fn a_conditional_exit_reaches_what_follows() {
        let source = "\
entity dut is
end entity dut;

architecture rtl of dut is
begin
  p : process is
    variable i : integer := 0;
  begin
    loop
      exit when i > 3;
      i := i + 1;
    end loop;
    wait;
  end process p;
end architecture rtl;
";
        assert!(
            check_source(source).is_empty(),
            "a conditional exit is not a transfer"
        );
    }
}

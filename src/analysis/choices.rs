//! `when others` on a case that has already named every value.
//!
//! A case statement over an enumeration must be exhaustive -- VHDL says so, and a front end
//! reports it when it is not. What VHDL does not say is anything about an `others` alternative
//! that follows a complete list of choices: it is legal, it compiles, and it can never be taken.
//!
//! It matters because of what happens next. Add a value to the enumeration, and the case is
//! exhaustive again for the wrong reason: the new value falls into `others`, silently doing
//! whatever that branch does -- commonly `null`, so the machine stalls in a state no one wrote a
//! transition for. Without the `others` the same edit is a compile error naming the choice that
//! is missing, which is the answer the author wanted.
//!
//! `lint_712` reports only where the branch is provably dead: the selector is a plain name of an
//! object declared with an enumeration this file declares, every value of that enumeration is a
//! plain identifier, and every choice in every other alternative is one of those values. Anything
//! this module cannot read -- a selector that is an expression, a choice that is a range or a
//! constant, a value that is a character literal -- disqualifies the whole case rather than being
//! guessed at.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::Parsed;
use vhdl_syntax::syntax::{NodeKind, SyntaxNode, SyntaxToken};

use super::design::{all_tokens, find, lower, text_of};
use super::lint::Finding;

/// The values of an enumeration, in declaration order, or `None` if any of them is not a plain
/// identifier. A type with character literals (`type t is ('0', '1', 'x')`) is readable VHDL but
/// not readable here, and a partial value set would make the exhaustiveness test a guess.
fn values_of(enumeration: &SyntaxNode) -> Option<Vec<String>> {
    let text = text_of(enumeration);
    let inner = text.trim().trim_start_matches('(').trim_end_matches(')');
    let mut values = Vec::new();
    for value in inner.split(',') {
        let value = value.trim().to_ascii_lowercase();
        let plain = value
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !plain {
            return None;
        }
        values.push(value);
    }
    (!values.is_empty()).then_some(values)
}

/// Every enumeration this architecture declares, by type name, and the objects declared with one.
///
/// Signals, variables and constants all qualify: the rule is about the choices a case lists, and
/// what kind of object is being cased on makes no difference to that.
type Enumerations = (
    BTreeMap<String, Vec<String>>,
    BTreeMap<String, Option<String>>,
);

fn enumerations(architecture: &SyntaxNode) -> Enumerations {
    let mut types: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
        let Some(values) = values_of(&enumeration) else {
            continue;
        };
        types.insert(name, values);
    }

    let mut objects: BTreeMap<String, Option<String>> = BTreeMap::new();
    for kind in [
        NodeKind::SignalDeclaration,
        NodeKind::VariableDeclaration,
        NodeKind::ConstantDeclaration,
    ] {
        for declaration in find(architecture, kind) {
            let text = text_of(&declaration);
            let Some((names, rest)) = text.split_once(':') else {
                continue;
            };
            // `names : mark := default;` -- the type mark is what precedes a default or a
            // constraint. `text_of` joins tokens without spaces, so there is nothing to strip.
            let mark = rest
                .trim_end_matches(';')
                .split([':', '('])
                .next()
                .unwrap_or(rest)
                .trim()
                .to_ascii_lowercase();
            if !types.contains_key(&mark) {
                continue;
            }
            for name in names
                .split(',')
                .map(|n| {
                    n.trim()
                        .trim_start_matches("signal")
                        .trim_start_matches("variable")
                        .trim_start_matches("constant")
                        .trim()
                        .to_ascii_lowercase()
                })
                .filter(|n| !n.is_empty())
            {
                // Two declarations of one name with different types -- a process variable
                // shadowing a signal, or two processes each with their own `state` -- leave the
                // selector ambiguous. Drop the name rather than let the last one win.
                match objects.entry(name) {
                    Entry::Vacant(slot) => {
                        slot.insert(Some(mark.clone()));
                    }
                    Entry::Occupied(mut slot) => {
                        if slot.get().as_ref() != Some(&mark) {
                            slot.insert(None);
                        }
                    }
                }
            }
        }
    }
    (types, objects)
}

/// The name a case selects on, if it is a plain name and nothing else. `case state is` gives
/// `state`; `case to_integer(count) is` and `case rec.field is` give `None`, because a selector
/// this module cannot resolve is a case it must not judge.
fn selector_of(case: &SyntaxNode) -> Option<String> {
    let preamble = find(case, NodeKind::CaseStatementPreamble)
        .into_iter()
        .next()?;
    let names: Vec<String> = all_tokens(&preamble)
        .iter()
        .map(lower)
        .filter(|t| t != "case" && t != "is")
        .collect();
    match names.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
}

/// Report every `when others` that no value of the selector's type can reach.
#[must_use]
pub fn check(parsed: &Parsed, file: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let at = |offset: usize| parsed.line_col(offset);
    for architecture in find(parsed.root(), NodeKind::ArchitectureBody) {
        let (types, objects) = enumerations(&architecture);
        if types.is_empty() || objects.is_empty() {
            continue;
        }

        for case in find(&architecture, NodeKind::CaseStatement) {
            let Some(values) = selector_of(&case)
                .and_then(|selector| objects.get(&selector).cloned().flatten())
                .and_then(|mark| types.get(&mark).cloned())
            else {
                continue;
            };

            let mut listed: BTreeSet<String> = BTreeSet::new();
            let mut others: Option<usize> = None;
            let mut readable = true;
            for alternative in find(&case, NodeKind::CaseStatementAlternative) {
                let Some(preamble) = find(&alternative, NodeKind::CaseStatementAlternativePreamble)
                    .into_iter()
                    .next()
                else {
                    continue;
                };
                let choices: Vec<String> = all_tokens(&preamble)
                    .iter()
                    .map(lower)
                    .filter(|t| t != "when" && t != "=>" && t != "|")
                    .collect();
                if choices.iter().any(|c| c == "others") {
                    // `when others` names nothing beside itself: `a | others` is not legal VHDL.
                    others = Some(
                        all_tokens(&preamble)
                            .first()
                            .map_or(0, SyntaxToken::text_offset),
                    );
                    continue;
                }
                // A choice that is not one of the type's values is a range, a constant or an
                // expression -- something whose coverage this module cannot compute.
                if choices.is_empty() || !choices.iter().all(|c| values.contains(c)) {
                    readable = false;
                    break;
                }
                listed.extend(choices);
            }

            let Some(offset) = others.filter(|_| readable) else {
                continue;
            };
            let (line, column) = at(offset);
            let selector = selector_of(&case).unwrap_or_default();
            // The two rules partition the `others` alternatives between them, so an alternative
            // is reported once whichever of them is enabled: `lint_712` takes the ones no value
            // reaches, `lint_713` the ones that do hide values.
            let hidden: Vec<&String> = values.iter().filter(|v| !listed.contains(*v)).collect();
            let finding = if hidden.is_empty() {
                Finding {
                    file: file.to_path_buf(),
                    rule: "lint_712",
                    line,
                    column,
                    message: format!(
                        "'when others' can never be taken: the other alternatives already name \
                         all {} values of '{selector}'. Removing it makes adding a value a \
                         compile error instead of a silent fall-through.",
                        values.len()
                    ),
                    related: Vec::new(),
                }
            } else {
                Finding {
                    file: file.to_path_buf(),
                    rule: "lint_713",
                    line,
                    column,
                    message: format!(
                        "'when others' covers {} of the values of '{selector}' ({}). Naming each \
                         one makes adding a value a compile error rather than a silent \
                         fall-through.",
                        hidden.len(),
                        hidden
                            .iter()
                            .map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    related: Vec::new(),
                }
            };
            findings.push(finding);
        }
    }
    findings.sort_by_key(|f| (f.line, f.column));
    findings
}

/// The rules this module reports, for `--list_rules`.
pub const RULES: &[super::Rule] = &[
    super::Rule {
        id: "lint_712",
        description: "A 'when others' alternative that no value can reach.",
        certainty: super::Certainty::Advisory,
    },
    super::Rule {
        id: "lint_713",
        description: "A 'when others' alternative on an enumeration, instead of naming every value \
         (off by default).",
        certainty: super::Certainty::Policy,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The messages of one rule only, since the two rules of this module partition the `others`
    /// alternatives between them.
    fn check_rule(source: &str, rule: &str) -> Vec<String> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        check(&parsed, Path::new("dut.vhd"))
            .into_iter()
            .filter(|f| f.rule == rule)
            .map(|f| f.message)
            .collect()
    }

    fn check_source(source: &str) -> Vec<String> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        check(&parsed, Path::new("dut.vhd"))
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    /// An architecture around `declarations` and `body`.
    fn architecture(declarations: &str, body: &str) -> String {
        format!(
            "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n{declarations}\
             begin\n  process (clk) is\n  begin\n{body}  end process;\nend architecture rtl;\n"
        )
    }

    const STATE: &str = "  type state_t is (idle, run, done);\n  signal state : state_t;\n";

    #[test]
    fn every_value_named_and_then_others() {
        let found = check_source(&architecture(
            STATE,
            "    case state is\n      when idle => null;\n      when run => null;\n      \
             when done => null;\n      when others => null;\n    end case;\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("can never be taken"), "{found:?}");
        assert!(found[0].contains("all 3 values of 'state'"), "{found:?}");
    }

    #[test]
    fn an_others_that_hides_values_is_the_policy_rule() {
        // `done` is only reachable through `others`. That is not a dead branch, so lint_712 has
        // nothing to say; it is lint_713's business, and lint_713 is off unless asked for.
        let found = check_rule(
            &architecture(
                STATE,
                "    case state is\n      when idle => null;\n      when run => null;\n      \
                 when others => null;\n    end case;\n",
            ),
            "lint_713",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("covers 1 of the values"), "{found:?}");
        assert!(found[0].contains("done"), "{found:?}");
    }

    #[test]
    fn an_exhaustive_case_without_others_is_not_reported() {
        let found = check_source(&architecture(
            STATE,
            "    case state is\n      when idle => null;\n      when run => null;\n      \
             when done => null;\n    end case;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn alternatives_may_group_their_choices() {
        let found = check_source(&architecture(
            STATE,
            "    case state is\n      when idle | run => null;\n      when done => null;\n      \
             when others => null;\n    end case;\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_case_on_something_else_is_left_alone() {
        // The selector is not an enumeration this file declares, so nothing is known about how
        // many values it has.
        let found = check_source(&architecture(
            "  signal byte : bit_vector(7 downto 0);\n",
            "    case byte is\n      when \"00000000\" => null;\n      when others => null;\n    \
             end case;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_choice_that_is_not_a_plain_value_disqualifies_the_case() {
        // `idle to done` is a range: whether the named choices cover the type is not something
        // this module works out, so it says nothing rather than guessing.
        let found = check_source(&architecture(
            STATE,
            "    case state is\n      when idle to done => null;\n      when others => null;\n    \
             end case;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_process_variable_of_an_architecture_type_counts() {
        let source = concat!(
            "entity dut is\n",
            "end entity dut;\n\n",
            "architecture rtl of dut is\n",
            "  type state_t is (idle, run, done);\n",
            "begin\n",
            "  process is\n",
            "    variable state : state_t;\n",
            "  begin\n",
            "    case state is\n",
            "      when idle => null;\n",
            "      when run => null;\n",
            "      when done => null;\n",
            "      when others => null;\n",
            "    end case;\n",
            "  end process;\n",
            "end architecture rtl;\n",
        );
        assert_eq!(check_source(source).len(), 1);
    }

    #[test]
    fn one_name_declared_with_two_types_is_ambiguous() {
        // Each process has its own `state`, of a different type. Which one the case selects is
        // not something a flat name map can answer, so neither case is judged.
        let source = concat!(
            "entity dut is\n",
            "end entity dut;\n\n",
            "architecture rtl of dut is\n",
            "  type small_t is (idle, run);\n",
            "  type big_t is (idle, run, done);\n",
            "begin\n",
            "  a : process is\n",
            "    variable state : small_t;\n",
            "  begin\n",
            "    case state is\n",
            "      when idle => null;\n",
            "      when run => null;\n",
            "      when others => null;\n",
            "    end case;\n",
            "  end process;\n",
            "  b : process is\n",
            "    variable state : big_t;\n",
            "  begin\n",
            "    case state is\n",
            "      when idle => null;\n",
            "      when others => null;\n",
            "    end case;\n",
            "  end process;\n",
            "end architecture rtl;\n",
        );
        let found = check_source(source);
        assert!(found.is_empty(), "{found:?}");
    }
}

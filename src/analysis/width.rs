//! Vector widths, and assignments between two that do not match.
//!
//! `a <= b` where `a` is eight bits and `b` is sixteen is legal VHDL: both are
//! `std_logic_vector`, so the type checker a front end gives you has nothing to say. The lengths
//! are compared only when the design elaborates, which means a simulator finds it and a reader
//! does not.
//!
//! `lint_740` compares them in the source, and only where the answer is certain: both sides a
//! plain name, both declared with a range of integer literals. A range mentioning a generic or a
//! constant is left alone, because vsg-rs does not evaluate those and a guess here would be a
//! false accusation about correct code.

use std::collections::BTreeMap;
use std::path::Path;

use crate::Parsed;
use vhdl_syntax::syntax::{NodeKind, SyntaxNode};

use super::design::{all_tokens, assignments, find, lower, path, reads};
use super::lint::Finding;

/// The width of a range such as `7downto0` -- `text_of` joins tokens without spaces, so the
/// keyword is not surrounded by any. `None` unless both bounds are integer literals.
fn width_of(range: &str) -> Option<u64> {
    let range = range.to_ascii_lowercase();
    let (left, right, descending) = match (range.split_once("downto"), range.split_once("to")) {
        (Some((left, right)), _) => (left, right, true),
        (None, Some((left, right))) => (left, right, false),
        (None, None) => return None,
    };
    let (left, right) = (
        left.trim().parse::<i64>().ok()?,
        right.trim().parse::<i64>().ok()?,
    );
    let span = if descending {
        left - right
    } else {
        right - left
    };
    u64::try_from(span + 1).ok()
}

/// Every object whose width this rule is sure of, by lower-case name. Read from the tokens
/// rather than the text, because the names and the range both need their own boundaries.
fn widths(node: &SyntaxNode, out: &mut BTreeMap<String, u64>) {
    for kind in [
        NodeKind::SignalDeclaration,
        NodeKind::InterfaceObjectDeclaration,
    ] {
        for declaration in find(node, kind) {
            let words: Vec<String> = all_tokens(&declaration).iter().map(lower).collect();
            let Some(colon) = words.iter().position(|w| w == ":") else {
                continue;
            };
            // `std_logic_vector ( 7 downto 0 )`, up to any default value.
            let rest = &words[colon + 1..];
            let end = rest.iter().position(|w| w == ":=").unwrap_or(rest.len());
            let rest = &rest[..end];
            let (Some(open), Some(close)) = (
                rest.iter().position(|w| w == "("),
                rest.iter().rposition(|w| w == ")"),
            ) else {
                continue;
            };
            let range = &rest[open + 1..close];
            // A nested parenthesis or a comma is an array of arrays or several dimensions: not
            // one range this rule can measure.
            if range.iter().any(|w| w == "(" || w == ",") {
                continue;
            }
            let Some(width) = width_of(&range.concat()) else {
                continue;
            };
            // `signal a, b : ...`: the names, without the class keyword or the mode.
            for name in words[..colon]
                .iter()
                .filter(|w| *w != "," && !KEYWORDS.contains(&w.as_str()))
            {
                // One name declared twice at two widths is not something to reason about.
                if out
                    .insert(name.clone(), width)
                    .is_some_and(|old| old != width)
                {
                    out.remove(name);
                }
            }
        }
    }
}

/// What can stand before the names in a declaration, and is not one.
const KEYWORDS: &[&str] = &["signal", "variable", "constant", "shared", "file"];

pub fn check(parsed: &Parsed, file: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut ports = BTreeMap::new();
    for entity in find(parsed.root(), NodeKind::EntityDeclaration) {
        widths(&entity, &mut ports);
    }
    for architecture in find(parsed.root(), NodeKind::ArchitectureBody) {
        let mut known = ports.clone();
        widths(&architecture, &mut known);
        if known.is_empty() {
            continue;
        }
        for kind in [
            NodeKind::ConcurrentSimpleSignalAssignment,
            NodeKind::SimpleWaveformAssignment,
        ] {
            for statement in find(&architecture, kind) {
                // `a <= b;` and nothing else: an operator, a concatenation or a conversion
                // gives the source a width this rule does not know.
                let words: Vec<String> = all_tokens(&statement)
                    .iter()
                    .map(lower)
                    .filter(|t| t != ";")
                    .collect();
                if words.len() != 3 || words[1] != "<=" {
                    continue;
                }
                let targets = assignments(&statement);
                let [(target, _)] = targets.as_slice() else {
                    continue;
                };
                let Some(left) = known.get(&path(target).0) else {
                    continue;
                };
                // `reads` can name one occurrence more than once; the statement is three
                // tokens, so there is at most one signal in it either way.
                let mut named = Vec::new();
                reads(&statement, &mut named);
                named.dedup();
                let [(source, offset)] = named.as_slice() else {
                    continue;
                };
                let Some(right) = known.get(source) else {
                    continue;
                };
                if left == right {
                    continue;
                }
                let (line, column) = parsed.line_col(*offset);
                findings.push(Finding {
                    file: file.to_path_buf(),
                    rule: "lint_740",
                    line,
                    column,
                    message: format!(
                        "'{source}' is {right} bits wide and is assigned to '{target}', \
                         which is {left}"
                    ),
                    related: Vec::new(),
                });
            }
        }
    }
    findings.sort_by_key(|f| (f.line, f.column));
    findings
}

/// The rules this module reports, for `--list_rules`.
pub const RULES: &[super::Rule] = &[super::Rule {
    id: "lint_740",
    description: "A vector is assigned to one of a different width.",
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

    fn architecture(declarations: &str, body: &str) -> String {
        format!(
            "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n{declarations}\
             \nbegin\n\n{body}\nend architecture rtl;\n"
        )
    }

    const TWO: &str = "  signal a : std_logic_vector(7 downto 0);\n  \
                       signal b : std_logic_vector(15 downto 0);\n";

    #[test]
    fn widths_are_read_from_the_range() {
        assert_eq!(width_of("7downto0"), Some(8));
        assert_eq!(width_of("0to3"), Some(4));
        assert_eq!(width_of("width_g-1downto0"), None);
        assert_eq!(width_of(""), None);
    }

    #[test]
    fn a_wider_source_is_reported() {
        let found = check_source(&architecture(TWO, "  a <= b;\n"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("16 bits"), "{found:?}");
    }

    #[test]
    fn matching_widths_are_quiet() {
        let found = check_source(&architecture(
            "  signal a : std_logic_vector(7 downto 0);\n  \
             signal b : std_logic_vector(7 downto 0);\n",
            "  a <= b;\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_generic_range_is_not_guessed_at() {
        let source = "entity dut is\n  generic (\n    width_g : positive := 8\n  );\n  \
                      port (\n    a : out std_logic_vector(width_g - 1 downto 0)\n  );\n\
                      end entity dut;\n\narchitecture rtl of dut is\n\n  \
                      signal b : std_logic_vector(15 downto 0);\n\nbegin\n\n  a <= b;\n\n\
                      end architecture rtl;\n";
        assert!(check_source(source).is_empty(), "the width is unknown");
    }

    #[test]
    fn a_slice_has_a_width_of_its_own() {
        let found = check_source(&architecture(TWO, "  a <= b(7 downto 0);\n"));
        assert!(
            found.is_empty(),
            "the slice is not the whole signal: {found:?}"
        );
    }

    #[test]
    fn an_expression_is_left_alone() {
        let found = check_source(&architecture(TWO, "  a <= not b;\n"));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_port_is_measured_too() {
        let source = "entity dut is\n  port (\n    q : out std_logic_vector(3 downto 0)\n  );\n\
                      end entity dut;\n\narchitecture rtl of dut is\n\n  \
                      signal b : std_logic_vector(15 downto 0);\n\nbegin\n\n  q <= b;\n\n\
                      end architecture rtl;\n";
        assert_eq!(check_source(source).len(), 1);
    }

    #[test]
    fn a_clocked_assignment_is_measured_too() {
        let found = check_source(&architecture(
            &format!("{TWO}  signal clk : std_logic;\n"),
            "  p : process (clk) is\n  begin\n    if rising_edge(clk) then\n      \
             a <= b;\n    end if;\n  end process p;\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
    }
}

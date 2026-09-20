//! What the source says outright cannot be evaluated.
//!
//! Both rules here report an operation that the program states in full: no generic decides it,
//! no constant has to be propagated to see it, and nothing about the design's intent changes
//! the answer. If the statement is reached, it raises an error. Nothing is reported when a
//! value the rule would need is not written down.
//!
//! ## Indexing outside the declared range
//!
//! `signal x : bit_vector(7 downto 0)` followed by `x(8)` names an element that does not exist.
//! The LRM makes the index subject to the array's index range, so evaluating it raises an error
//! -- there is no reading under which the program carries on. Both simulators agree, and both
//! say it at analysis rather than waiting for the run: GHDL reports "static expression violates
//! bounds" and NVC "array X index 8 outside of NATURAL range 7 downto 0". The VHDL front end
//! vsg-rs uses reports neither, which is why this rule exists.
//!
//! Only the exact case is reported: the object's range is written as integer literals and the
//! index is an integer literal. A range mentioning a generic is not evaluated, an index that is
//! an expression is not evaluated, and neither is reported on. There is no constant propagation
//! here and no attempt at one -- the rule reports what the source states outright and stays
//! quiet about the rest.
//!
//! ## Dividing by zero
//!
//! `x / 0`, `x mod 0` and `x rem 0` each raise an error when evaluated: the LRM leaves the
//! result undefined for a zero right operand and requires the error. Unlike the index case,
//! neither GHDL nor NVC says anything about it at analysis, so nothing warns before the run
//! reaches the statement.
//!
//! Only a literal zero is read. A named constant that happens to be zero would need its value
//! propagated, and propagating values is how a rule stops being able to say what it knows.

use std::collections::BTreeMap;
use std::path::Path;

use crate::Parsed;
use vhdl_syntax::syntax::{NodeKind, SyntaxNode};

use super::design::{all_tokens, find, text_of};
use super::lint::Finding;

/// The bounds of a range written as two integer literals: `7 downto 0` is `(0, 7)`.
///
/// `text_of` joins tokens without whitespace, so the keyword has nothing around it.
fn literal_bounds(text: &str) -> Option<(i64, i64)> {
    let text = text.trim().trim_start_matches('(').trim_end_matches(')');
    let lower = text.to_ascii_lowercase();
    let (left, right) = lower
        .split_once("downto")
        .or_else(|| lower.split_once("to"))?;
    let (left, right) = (
        left.trim().parse::<i64>().ok()?,
        right.trim().parse::<i64>().ok()?,
    );
    Some((left.min(right), left.max(right)))
}

/// Every object declared with a range this module can read, and that range.
///
/// A name declared twice with different ranges is dropped: which one an index belongs to is a
/// question about scope, and answering it wrongly means accusing correct code.
fn declared_ranges(root: &SyntaxNode) -> BTreeMap<String, (i64, i64)> {
    let mut found: BTreeMap<String, Option<(i64, i64)>> = BTreeMap::new();
    for kind in [
        NodeKind::SignalDeclaration,
        NodeKind::VariableDeclaration,
        NodeKind::ConstantDeclaration,
        NodeKind::InterfaceObjectDeclaration,
    ] {
        for declaration in find(root, kind) {
            let text = text_of(&declaration);
            let Some((names, rest)) = text.split_once(':') else {
                continue;
            };
            // The constraint belongs to the type mark: `bit_vector(7downto0)`.
            let Some(bounds) = rest
                .split_once('(')
                .and_then(|(_, tail)| tail.split_once(')'))
                .and_then(|(range, _)| literal_bounds(range))
            else {
                continue;
            };
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
                match found.get(&name) {
                    Some(Some(known)) if *known != bounds => {
                        found.insert(name, None);
                    }
                    Some(_) => {}
                    None => {
                        found.insert(name, Some(bounds));
                    }
                }
            }
        }
    }
    found
        .into_iter()
        .filter_map(|(name, bounds)| Some((name, bounds?)))
        .collect()
}

/// Report every index the source states outright is outside its object's range.
#[must_use]
pub fn check(parsed: &Parsed, file: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    let ranges = declared_ranges(parsed.root());

    for name in find(parsed.root(), NodeKind::Name) {
        let parts: Vec<SyntaxNode> = name.children().collect();
        // `x(8)`: a prefix and one parenthesised part. Anything else -- a selected name, a
        // second index, a slice of a slice -- is a shape this module does not read.
        let [prefix, index] = parts.as_slice() else {
            continue;
        };
        if prefix.kind() != NodeKind::NameDesignatorPrefix
            || index.kind() != NodeKind::ParenthesizedName
        {
            continue;
        }
        let object = text_of(prefix).trim().to_ascii_lowercase();
        let Some(&(low, high)) = ranges.get(&object) else {
            continue;
        };
        // A plain integer and nothing else. `x(i)`, `x(i + 1)` and `x(3 downto 0)` each state
        // nothing this rule can check, so each is left alone.
        let Ok(at) = text_of(index)
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim()
            .parse::<i64>()
        else {
            continue;
        };
        if at >= low && at <= high {
            continue;
        }
        let Some(token) = all_tokens(&name).first().cloned() else {
            continue;
        };
        let (line, column) = parsed.line_col(token.text_offset());
        findings.push(Finding {
            file: file.to_path_buf(),
            rule: "lint_780",
            line,
            column,
            message: format!(
                "Index {at} is outside the range {low} to {high} of '{object}'. Evaluating this \
                 raises an error: an index has to be within the array's range."
            ),
            related: Vec::new(),
        });
    }
    divisions(parsed, file, &mut findings);
    findings.sort_by_key(|f| (f.line, f.column));
    findings
}

/// Report every division whose divisor is written as zero.
fn divisions(parsed: &Parsed, file: &Path, findings: &mut Vec<Finding>) {
    let tokens = all_tokens(parsed.root());
    for pair in tokens.windows(2) {
        let [operator, divisor] = pair else {
            continue;
        };
        let name = match operator.text().to_string().to_ascii_lowercase().as_str() {
            "/" => "Division",
            "mod" => "`mod`",
            "rem" => "`rem`",
            _ => continue,
        };
        // A literal zero, however it is written. A name, a parenthesis or anything else that is
        // not a number states nothing this rule can check.
        let Ok(value) = divisor.text().to_string().parse::<f64>() else {
            continue;
        };
        if value != 0.0 {
            continue;
        }
        let (line, column) = parsed.line_col(divisor.text_offset());
        findings.push(Finding {
            file: file.to_path_buf(),
            rule: "lint_781",
            line,
            column,
            message: format!(
                "{name} by zero. Evaluating this raises an error: the right operand of `/`, \
                 `mod` and `rem` may not be zero."
            ),
            related: Vec::new(),
        });
    }
}

/// The rules this module reports, for `--list_rules`.
pub const RULES: &[super::Rule] = &[
    super::Rule {
        id: "lint_780",
        description: "An index that is outside the declared range of the array it selects from.",
        certainty: super::Certainty::Definite,
    },
    super::Rule {
        id: "lint_781",
        description: "A division, `mod` or `rem` whose divisor is written as zero.",
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
            .map(|f| f.message)
            .collect()
    }

    /// An architecture declaring `declarations` whose body is `body`.
    fn architecture(declarations: &str, body: &str) -> String {
        format!(
            "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n{declarations}\
             begin\n{body}end architecture rtl;\n"
        )
    }

    const VECTOR: &str = "  signal x : bit_vector(7 downto 0);\n  signal y : bit;\n";

    #[test]
    fn an_index_past_the_end() {
        let found = check_source(&architecture(VECTOR, "  y <= x(8);\n"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("Index 8 is outside"), "{found:?}");
        assert!(found[0].contains("'x'"), "{found:?}");
    }

    #[test]
    fn an_index_below_the_start() {
        let found = check_source(&architecture(
            "  signal x : bit_vector(3 to 7);\n  signal y : bit;\n",
            "  y <= x(2);\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn every_index_in_range_is_silent() {
        let found = check_source(&architecture(VECTOR, "  y <= x(7) xor x(0) xor x(3);\n"));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn an_index_that_is_not_a_literal_is_not_evaluated() {
        // The answer depends on a value the rule does not have, so it does not invent one.
        let found = check_source(&architecture(
            "  signal x : bit_vector(7 downto 0);\n  signal y : bit;\n  \
             signal i : integer := 9;\n",
            "  y <= x(i);\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_range_that_is_not_literal_is_not_evaluated() {
        // A generic decides this range. Evaluating it would mean evaluating generics, which is
        // exactly the guessing this layer refuses.
        let source = "entity dut is\n  generic (n : integer := 4);\nend entity dut;\n\n\
                      architecture rtl of dut is\n  signal x : bit_vector(n - 1 downto 0);\n  \
                      signal y : bit;\nbegin\n  y <= x(8);\nend architecture rtl;\n";
        assert!(check_source(source).is_empty());
    }

    #[test]
    fn a_slice_is_not_an_index() {
        let found = check_source(&architecture(
            "  signal x : bit_vector(7 downto 0);\n  signal z : bit_vector(3 downto 0);\n",
            "  z <= x(3 downto 0);\n",
        ));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_name_declared_twice_with_different_ranges_is_left_alone() {
        let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n  \
                      signal x : bit_vector(7 downto 0);\n  signal y : bit;\nbegin\n  \
                      p : process is\n    variable x : bit_vector(15 downto 0);\n  begin\n    \
                      y <= x(8);\n    wait;\n  end process p;\nend architecture rtl;\n";
        assert!(check_source(source).is_empty());
    }

    #[test]
    fn a_function_call_is_not_an_index() {
        // `f(8)` looks exactly like an index. It is read as one only when the prefix is an
        // object this file declares with a range.
        let found = check_source(&architecture(VECTOR, "  y <= f(8);\n"));
        assert!(found.is_empty(), "{found:?}");
    }
}

#[cfg(test)]
mod divisor_tests {
    use super::*;

    fn check_source(source: &str) -> Vec<String> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        check(&parsed, Path::new("dut.vhd"))
            .into_iter()
            .filter(|f| f.rule == "lint_781")
            .map(|f| f.message)
            .collect()
    }

    /// A process whose body is `body`.
    fn process(body: &str) -> String {
        format!(
            "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\nbegin\n  \
             p : process is\n    variable n : integer := 7;\n  begin\n{body}    wait;\n  \
             end process p;\nend architecture rtl;\n"
        )
    }

    #[test]
    fn each_operator_with_a_zero_divisor() {
        let found = check_source(&process(
            "    n := n / 0;\n    n := n mod 0;\n    n := n rem 0;\n",
        ));
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].starts_with("Division by zero"), "{found:?}");
        assert!(found[1].contains("`mod` by zero"), "{found:?}");
        assert!(found[2].contains("`rem` by zero"), "{found:?}");
    }

    #[test]
    fn a_divisor_that_is_not_zero_is_silent() {
        let found = check_source(&process("    n := n / 2;\n    n := n mod 8;\n"));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_named_divisor_is_not_evaluated() {
        // `zero` is zero, and seeing that needs the value propagated to the division. That is
        // the machinery this layer does without, so it says nothing.
        let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n  \
                      constant zero : integer := 0;\nbegin\n  p : process is\n    \
                      variable n : integer := 7;\n  begin\n    n := n / zero;\n    wait;\n  \
                      end process p;\nend architecture rtl;\n";
        assert!(check_source(source).is_empty());
    }

    #[test]
    fn inequality_is_not_a_division() {
        // `/=` is one token, not `/` followed by something.
        let found = check_source(&process("    if n /= 0 then\n      n := 1;\n    end if;\n"));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_zero_that_is_not_a_divisor_is_silent() {
        let found = check_source(&process("    n := n + 0;\n    n := n - 0;\n"));
        assert!(found.is_empty(), "{found:?}");
    }
}

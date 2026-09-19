//! Clock domains, and signals that cross between them.
//!
//! A register belongs to the clock its process is edge-triggered on. When logic in one domain
//! reads a register from another, the value can be sampled while it is changing, and the result
//! is metastable: the classic bug a designer buys a lint tool to find.
//!
//! `lint_700` reports a crossing that feeds logic. A crossing that is only *captured* — a plain
//! `flop <= other_domain_signal;` and nothing else in the statement — is the first stage of a
//! synchroniser and is accepted, as is anything touching an entity named in
//! `vsg_rs: synchronizers`. Between them those cover how a crossing is normally made safe, and
//! leave the unsafe shape: a signal from another clock used in an expression.
//!
//! It is a deliberate under-report. A single-stage capture is accepted although two stages are
//! the usual requirement, because a false accusation about a correct synchroniser costs more
//! than the finding is worth.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::Parsed;
use vhdl_syntax::syntax::{NodeKind, SyntaxNode};

use super::design::{all_tokens, assignments, entity_of, find, lower, path, reads};
use super::lint::Finding;

/// The lower-case text of every token under a node, with semicolons dropped.
fn words(node: &SyntaxNode) -> Vec<String> {
    all_tokens(node)
        .iter()
        .map(lower)
        .filter(|t| t != ";")
        .collect()
}

/// The clock a process is triggered on, if it is a register.
fn clock_of(process: &SyntaxNode) -> Option<String> {
    let tokens = words(process);
    for (at, token) in tokens.iter().enumerate() {
        if token == "rising_edge" || token == "falling_edge" {
            // `rising_edge ( clk )`
            return tokens.get(at + 2).cloned();
        }
        // `clk'event`, however the lexer splits the tick.
        if token == "'event" {
            return at
                .checked_sub(1)
                .and_then(|before| tokens.get(before))
                .cloned();
        }
        if token == "'" && tokens.get(at + 1).is_some_and(|t| t == "event") {
            return at
                .checked_sub(1)
                .and_then(|before| tokens.get(before))
                .cloned();
        }
    }
    None
}

/// Whether a statement only captures one signal into another: `flop <= source;` and nothing
/// else. That is the first stage of a synchroniser, and is how a crossing is made safe.
fn is_plain_capture(statement: &SyntaxNode, source: &str) -> bool {
    let tokens = words(statement);
    tokens.len() == 3 && tokens[1] == "<=" && tokens[2] == source
}

/// Signals that reach an instance of an entity the project calls a synchroniser. Whatever goes
/// through one is being synchronised on purpose.
fn through_synchronizers(architecture: &SyntaxNode, patterns: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if patterns.is_empty() {
        return out;
    }
    for instance in find(architecture, NodeKind::ComponentInstantiationStatement) {
        let entity = entity_of(&instance).unwrap_or_default();
        if !patterns
            .iter()
            .any(|pattern| crate::config::glob(pattern.as_bytes(), entity.as_bytes()))
        {
            continue;
        }
        let mut named = Vec::new();
        reads(&instance, &mut named);
        out.extend(named.into_iter().map(|(name, _)| name));
        out.extend(assignments(&instance).iter().map(|(t, _)| path(t).0));
    }
    out
}

/// The assignment kinds inside a process that can read another domain.
const ASSIGNMENTS: &[NodeKind] = &[
    NodeKind::SimpleWaveformAssignment,
    NodeKind::ConditionalWaveformAssignment,
    NodeKind::SelectedWaveformAssignment,
];

pub fn check(parsed: &Parsed, file: &Path, synchronizers: &[String]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for architecture in find(parsed.root(), NodeKind::ArchitectureBody) {
        let processes = find(&architecture, NodeKind::ProcessStatement);
        // Which clock each register belongs to. A signal registered on two clocks is not a
        // crossing anyone can reason about, so it is dropped.
        let mut domain: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for process in &processes {
            let Some(clock) = clock_of(process) else {
                continue;
            };
            for (target, _) in assignments(process) {
                domain
                    .entry(path(&target).0)
                    .or_default()
                    .insert(clock.clone());
            }
        }
        domain.retain(|_, clocks| clocks.len() == 1);
        if domain.values().flatten().collect::<BTreeSet<_>>().len() < 2 {
            // One clock: nothing can cross.
            continue;
        }
        let safe = through_synchronizers(&architecture, synchronizers);

        for process in &processes {
            let Some(clock) = clock_of(process) else {
                continue;
            };
            for kind in ASSIGNMENTS {
                for statement in find(process, *kind) {
                    let mut named = Vec::new();
                    reads(&statement, &mut named);
                    for (name, offset) in named {
                        if name == clock || safe.contains(&name) {
                            continue;
                        }
                        let Some(other) = domain
                            .get(&name)
                            .and_then(|clocks| clocks.iter().next())
                            .filter(|other| **other != clock)
                        else {
                            continue;
                        };
                        // Captured into a flop and nothing else: a synchroniser's first stage.
                        if is_plain_capture(&statement, &name) {
                            continue;
                        }
                        let (line, column) = parsed.line_col(offset);
                        findings.push(Finding {
                            file: file.to_path_buf(),
                            rule: "lint_700",
                            line,
                            column,
                            message: format!(
                                "Signal '{name}' is registered on '{other}' and used in logic on \
                                 '{clock}': an unsynchronised clock domain crossing"
                            ),
                            related: Vec::new(),
                        });
                    }
                }
            }
        }
    }
    findings.sort_by_key(|f| (f.line, f.column));
    findings.dedup_by(|a, b| a.line == b.line && a.message == b.message);
    findings
}

/// The rules this module reports, for `--list_rules`.
pub const RULES: &[(&str, &str)] = &[(
    "lint_700",
    "A signal registered on one clock is used in logic on another, without a synchroniser.",
)];

#[cfg(test)]
mod tests {
    use super::*;

    fn check_source(source: &str, synchronizers: &[&str]) -> Vec<String> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        let patterns: Vec<String> = synchronizers.iter().map(|s| (*s).to_owned()).collect();
        check(&parsed, Path::new("dut.vhd"), &patterns)
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    fn two_clocks(body: &str) -> String {
        format!(
            "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  \
             signal clk_a, clk_b, a_data, b_data, b_sync, y : bit;\n\nbegin\n\n  \
             a : process (clk_a) is\n  begin\n    if rising_edge(clk_a) then\n      \
             a_data <= not a_data;\n    end if;\n  end process a;\n\n{body}\n\
             end architecture rtl;\n"
        )
    }

    fn in_b(body: &str) -> String {
        two_clocks(&format!(
            "  b : process (clk_b) is\n  begin\n    if rising_edge(clk_b) then\n      \
             {body}\n    end if;\n  end process b;\n"
        ))
    }

    #[test]
    fn using_another_domains_register_in_logic_is_reported() {
        let found = check_source(&in_b("y <= a_data and b_data;"), &[]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("'a_data'"), "{found:?}");
    }

    #[test]
    fn capturing_it_into_a_flop_is_a_synchroniser() {
        let found = check_source(&in_b("b_sync <= a_data;"), &[]);
        assert!(
            found.is_empty(),
            "a plain capture is the first stage: {found:?}"
        );
    }

    #[test]
    fn staying_in_one_domain_is_quiet() {
        let found = check_source(&in_b("y <= b_data and y;"), &[]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn one_clock_cannot_cross() {
        let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  \
                      signal clk, a, y : bit;\n\nbegin\n\n  p : process (clk) is\n  begin\n    \
                      if rising_edge(clk) then\n      a <= not a;\n      y <= a and y;\n    \
                      end if;\n  end process p;\n\nend architecture rtl;\n";
        assert!(check_source(source, &[]).is_empty());
    }

    #[test]
    fn a_synchroniser_is_named_by_its_entity_and_nothing_else() {
        // The name matched against `synchronizers` is the entity's, not the whole statement's
        // text. It used to be taken from the statement, so it carried the port map with it and
        // only a pattern ending in `*` ever matched; an exact name silently did not.
        let body = "  u : entity work.cdc_bit_sync\n    port map (\n      d => a_data,\n      \
                    q => b_sync\n    );\n\n  b : process (clk_b) is\n  begin\n    \
                    if rising_edge(clk_b) then\n      y <= a_data and b_data;\n    end if;\n  \
                    end process b;\n";
        assert!(
            check_source(&two_clocks(body), &["cdc_bit_sync"]).is_empty(),
            "an exact entity name must match"
        );
    }

    #[test]
    fn a_named_synchroniser_makes_it_safe() {
        let body = "  u : entity work.cdc_bit_sync\n    port map (\n      d => a_data,\n      \
                    q => b_sync\n    );\n\n  b : process (clk_b) is\n  begin\n    \
                    if rising_edge(clk_b) then\n      y <= a_data and b_data;\n    end if;\n  \
                    end process b;\n";
        assert!(
            check_source(&two_clocks(body), &["cdc_*"]).is_empty(),
            "the project named it a synchroniser"
        );
        assert_eq!(
            check_source(&two_clocks(body), &[]).len(),
            1,
            "without the configuration it is still a crossing"
        );
    }
}

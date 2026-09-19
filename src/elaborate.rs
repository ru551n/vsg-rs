//! What the design is wired like, without synthesising it.
//!
//! The checks in `design.rs` look at one architecture at a time. The ones a commercial linter is
//! bought for — a signal with no driver, a clock domain crossing, an unreachable state — need to
//! know what an instance does to the signals connected to it, and that needs the entity on the
//! other side of the port map. This module builds that much: every entity's ports and their
//! modes, gathered from the whole run, so a port map can be read as "these signals are driven,
//! those are read".
//!
//! It is deliberately not elaboration in the LRM sense: no generics are folded and no hierarchy
//! is instantiated. It is the least cross-file knowledge that makes wiring checks possible, and
//! whatever it cannot resolve is treated as unknown rather than guessed at.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use vhdl_syntax::syntax::{NodeKind, SyntaxNode};
use vsg_rs::Parsed;

use crate::design::{all_tokens, assignments, find, path, reads, text_of};
use crate::lint::Finding;

/// The ports of one entity, by what they do to a signal connected to them.
#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Ports {
    /// Ports that drive their actual: `out`, `inout`, `buffer`.
    pub(crate) driving: BTreeSet<String>,
    /// Ports that read their actual: `in`, `inout`.
    pub(crate) reading: BTreeSet<String>,
    /// Declaration order, for positional association.
    pub(crate) order: Vec<String>,
}

/// Every entity the run can see, by lower-case name.
pub(crate) type Entities = BTreeMap<String, Ports>;

fn lower(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

/// The ports of one entity or component declaration.
fn ports_of(node: &SyntaxNode) -> Ports {
    let mut ports = Ports::default();
    for clause in find(node, NodeKind::PortClause) {
        for declaration in find(&clause, NodeKind::InterfaceObjectDeclaration) {
            let text = text_of(&declaration);
            // `a, b : out std_logic` declares two ports of one mode.
            let Some((names, rest)) = text.split_once(':') else {
                continue;
            };
            let driving =
                rest.starts_with("out") || rest.starts_with("inout") || rest.starts_with("buffer");
            let reading =
                rest.starts_with("inout") || (rest.starts_with("in") && !rest.starts_with("inout"));
            for name in names.split(',').map(lower).filter(|n| !n.is_empty()) {
                if driving {
                    ports.driving.insert(name.clone());
                }
                // A port with no mode written is an input.
                if reading || !driving {
                    ports.reading.insert(name.clone());
                }
                ports.order.push(name);
            }
        }
    }
    ports
}

/// Read every input once and keep only what a port map needs: entity names and port modes. The
/// syntax trees are dropped again, so this costs a parse rather than the memory of the design.
pub(crate) fn entities(files: &[PathBuf]) -> Entities {
    let mut out = Entities::new();
    for file in files {
        let Ok(source) = std::fs::read(file) else {
            continue;
        };
        let parsed = Parsed::new(source);
        if !parsed.syntax_errors().is_empty() {
            continue;
        }
        for kind in [NodeKind::EntityDeclaration, NodeKind::ComponentDeclaration] {
            for declaration in find(parsed.root(), kind) {
                let Some(name) = all_tokens(&declaration)
                    .iter()
                    .map(|t| String::from_utf8_lossy(t.text().as_bytes()).to_ascii_lowercase())
                    .find(|t| t != "entity" && t != "component")
                else {
                    continue;
                };
                // A component declaration repeats an entity; whichever is seen first wins, and
                // they have to agree anyway.
                out.entry(name).or_insert_with(|| ports_of(&declaration));
            }
        }
    }
    out
}

/// One instantiation: what it instantiates, and what is connected to it.
struct Instance {
    entity: String,
    /// `(formal, actual)`; the formal is `None` for a positional association.
    associations: Vec<(Option<String>, String)>,
}

fn instantiations(architecture: &SyntaxNode) -> Vec<Instance> {
    let mut out = Vec::new();
    for statement in find(architecture, NodeKind::ComponentInstantiationStatement) {
        // `entity work.fifo(rtl)`, `component fifo` or a bare name: the entity is the last
        // identifier before any architecture in parentheses.
        let Some(instantiated) = statement
            .children()
            .find(|c| c.kind() == NodeKind::InstantiatedEntity)
        else {
            continue;
        };
        let text = text_of(&instantiated);
        let head = text.split('(').next().unwrap_or(&text);
        let Some(entity) = head.rsplit(['.', ' ']).map(lower).find(|part| {
            !part.is_empty()
                && !matches!(part.as_str(), "entity" | "component" | "configuration")
                && part.chars().all(|c| c.is_alphanumeric() || c == '_')
        }) else {
            continue;
        };
        let mut associations = Vec::new();
        for map in find(&statement, NodeKind::PortMapAspect) {
            for association in find(&map, NodeKind::AssociationElement) {
                // The `Formal` node carries the `=>` with it.
                let formal = association
                    .children()
                    .find(|c| c.kind() == NodeKind::Formal)
                    .map(|formal| lower(text_of(&formal).trim_end_matches("=>")));
                let actual: String = association
                    .children()
                    .filter(|c| c.kind() != NodeKind::Formal)
                    .map(|c| text_of(&c))
                    .collect();
                associations.push((formal, lower(&actual)));
            }
        }
        out.push(Instance {
            entity,
            associations,
        });
    }
    out
}

/// `lint_730`: a signal that something reads but nothing drives.
///
/// In simulation it keeps its initial value and in synthesis it becomes a constant, so it is
/// nearly always a wiring mistake. A signal connected to an instance whose entity the run cannot
/// see is left alone: without the port modes there is no telling a driver from a reader, and
/// guessing would accuse correct code.
pub(crate) fn undriven(parsed: &Parsed, file: &Path, entities: &Entities) -> Vec<Finding> {
    let mut findings = Vec::new();
    for architecture in find(parsed.root(), NodeKind::ArchitectureBody) {
        let mut declared: BTreeMap<String, usize> = BTreeMap::new();
        for part in find(&architecture, NodeKind::ArchitectureDeclarativePart) {
            for declaration in find(&part, NodeKind::SignalDeclaration) {
                // `signal tie : std_logic := '0';` holds its value on purpose; a signal that is
                // tied rather than driven is a decision, not a missing driver.
                if text_of(&declaration).contains(":=") {
                    continue;
                }
                for token in all_tokens(&declaration) {
                    let text =
                        String::from_utf8_lossy(token.text().as_bytes()).to_ascii_lowercase();
                    if text == ":" {
                        break;
                    }
                    if text != "signal" && text != "," {
                        declared.insert(text, token.text_offset());
                    }
                }
            }
        }
        if declared.is_empty() {
            continue;
        }

        let mut driven: BTreeSet<String> = BTreeSet::new();
        let mut read: BTreeSet<String> = BTreeSet::new();
        for (target, _) in assignments(&architecture) {
            driven.insert(path(&target).0);
        }
        let mut named = Vec::new();
        reads(&architecture, &mut named);
        read.extend(named.into_iter().map(|(name, _)| name));

        // A procedure drives whatever it takes as an `out` or `inout` parameter, and vsg-rs
        // cannot see subprogram signatures: every name a call mentions is treated as driven.
        // The same goes for a concurrent statement the parser could not tell apart from an
        // instantiation, which is how a concurrent procedure call reaches here.
        for kind in [
            NodeKind::ProcedureCallStatement,
            NodeKind::ConcurrentProcedureCallOrComponentInstantiationStatement,
        ] {
            for call in find(&architecture, kind) {
                let mut mentioned = Vec::new();
                reads(&call, &mut mentioned);
                driven.extend(mentioned.into_iter().map(|(name, _)| name));
                for (target, _) in assignments(&call) {
                    driven.insert(path(&target).0);
                }
            }
        }

        for instance in instantiations(&architecture) {
            let Some(ports) = entities.get(&instance.entity) else {
                // An entity the run cannot see: treat everything it touches as driven, so
                // nothing is reported about it.
                for (_, actual) in &instance.associations {
                    driven.insert(path(actual).0);
                }
                continue;
            };
            for (at, (formal, actual)) in instance.associations.iter().enumerate() {
                let port = match formal {
                    Some(name) => name.clone(),
                    None => ports.order.get(at).cloned().unwrap_or_default(),
                };
                let actual = path(actual).0;
                if ports.driving.contains(&port) {
                    driven.insert(actual.clone());
                }
                if ports.reading.contains(&port) {
                    read.insert(actual);
                } else if !ports.driving.contains(&port) {
                    // A formal the entity does not have: the port map is wrong, which
                    // `lint_402` reports. Do not build another finding on top of it.
                    driven.insert(actual);
                }
            }
        }

        for (signal, offset) in declared {
            if driven.contains(&signal) || !read.contains(&signal) {
                continue;
            }
            let (line, column) = parsed.line_col(offset);
            findings.push(Finding {
                file: file.to_path_buf(),
                rule: "lint_730",
                line,
                column,
                message: format!("Signal '{signal}' is read but nothing drives it"),
                related: Vec::new(),
            });
        }
    }
    findings.sort_by_key(|f| (f.line, f.column));
    findings
}

/// The rules this module reports, for `--list_rules`.
pub(crate) const RULES: &[(&str, &str)] = &[(
    "lint_730",
    "A signal is read but nothing drives it: no assignment, and no instance output.",
)];

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str, entities: &Entities) -> Vec<String> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        undriven(&parsed, Path::new("dut.vhd"), entities)
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    fn fifo() -> Entities {
        let source = "entity fifo is\n  port (\n    wr : in bit;\n    rd : out bit\n  );\n\
                      end entity fifo;\n";
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("fifo.vhd");
        std::fs::write(&path, source).expect("write");
        entities(std::slice::from_ref(&path))
    }

    #[test]
    fn ports_are_read_by_their_mode() {
        let found = fifo();
        let ports = found.get("fifo").expect("the entity was found");
        assert!(ports.driving.contains("rd"), "{ports:?}");
        assert!(ports.reading.contains("wr"), "{ports:?}");
        assert_eq!(ports.order, ["wr", "rd"]);
    }

    #[test]
    fn a_signal_nothing_drives_is_reported() {
        let messages = check(
            "entity dut is\n  port (\n    y : out bit\n  );\nend entity dut;\n\n\
             architecture rtl of dut is\n\n  signal orphan : bit;\n\nbegin\n\n  \
             y <= orphan;\n\nend architecture rtl;\n",
            &Entities::new(),
        );
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("'orphan'"), "{messages:?}");
    }

    #[test]
    fn an_instance_output_is_a_driver() {
        let messages = check(
            "entity dut is\n  port (\n    y : out bit\n  );\nend entity dut;\n\n\
             architecture rtl of dut is\n\n  signal s : bit;\n\nbegin\n\n  \
             u : entity work.fifo\n    port map (\n      wr => y,\n      rd => s\n    );\n\n  \
             y <= s;\n\nend architecture rtl;\n",
            &fifo(),
        );
        assert!(messages.is_empty(), "the instance drives s: {messages:?}");
    }

    #[test]
    fn an_instance_input_does_not_drive() {
        let messages = check(
            "entity dut is\n  port (\n    y : out bit\n  );\nend entity dut;\n\n\
             architecture rtl of dut is\n\n  signal s : bit;\n\nbegin\n\n  \
             u : entity work.fifo\n    port map (\n      wr => s,\n      rd => y\n    );\n\n  \
             y <= s;\n\nend architecture rtl;\n",
            &fifo(),
        );
        assert_eq!(messages.len(), 1, "s is only read: {messages:?}");
    }

    #[test]
    fn an_unknown_entity_is_left_alone() {
        let messages = check(
            "entity dut is\n  port (\n    y : out bit\n  );\nend entity dut;\n\n\
             architecture rtl of dut is\n\n  signal s : bit;\n\nbegin\n\n  \
             u : entity work.mystery\n    port map (\n      a => s\n    );\n\n  \
             y <= s;\n\nend architecture rtl;\n",
            &Entities::new(),
        );
        assert!(
            messages.is_empty(),
            "without the entity there is no telling: {messages:?}"
        );
    }

    #[test]
    fn a_signal_tied_by_its_initial_value_is_not_reported() {
        let messages = check(
            "entity dut is\n  port (\n    y : out bit\n  );\nend entity dut;\n\n\
             architecture rtl of dut is\n\n  signal tied : bit := '0';\n\nbegin\n\n  \
             y <= tied;\n\nend architecture rtl;\n",
            &Entities::new(),
        );
        assert!(messages.is_empty(), "tied on purpose: {messages:?}");
    }

    #[test]
    fn a_signal_nobody_reads_is_not_this_rule() {
        let messages = check(
            "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  \
             signal unused : bit;\n\nbegin\n\nend architecture rtl;\n",
            &Entities::new(),
        );
        assert!(messages.is_empty(), "that is lint_004: {messages:?}");
    }
}

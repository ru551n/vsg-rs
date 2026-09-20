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

use crate::Parsed;
use vhdl_syntax::syntax::{NodeKind, SyntaxNode};

use super::design::{all_tokens, assignments, entity_of, find, lower, path, reads, text_of};
use super::lint::Finding;

/// The ports of one entity, by what they do to a signal connected to them.
///
/// `#[non_exhaustive]`: this grows as the wiring checks learn to ask more of an interface, and a
/// new field should not be a breaking change. Nothing outside this crate builds one.
#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct Ports {
    /// Ports that drive their actual: `out`, `inout`, `buffer`.
    pub driving: BTreeSet<String>,
    /// Ports that read their actual: `in`, `inout`.
    pub reading: BTreeSet<String>,
    /// Declaration order, for positional association.
    pub order: Vec<String>,
    /// Whether this came from the entity itself rather than a component declaration repeating
    /// it. The entity is the authority when the two disagree, which is what `lint_750` reports.
    #[serde(default)]
    pub from_entity: bool,
}

/// Every entity the run can see, by lower-case name.
pub type Entities = BTreeMap<String, Ports>;

/// A name as it is compared here: trimmed, and lower case because VHDL is case-insensitive.
fn trimmed(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

/// The ports of one entity or component declaration.
fn ports_of(node: &SyntaxNode, from_entity: bool) -> Ports {
    let mut ports = Ports {
        from_entity,
        ..Ports::default()
    };
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
            for name in names.split(',').map(trimmed).filter(|n| !n.is_empty()) {
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
pub fn entities(files: &[PathBuf]) -> Entities {
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
            let from_entity = kind == NodeKind::EntityDeclaration;
            for declaration in find(parsed.root(), kind) {
                let Some(name) = all_tokens(&declaration)
                    .iter()
                    .map(lower)
                    .find(|t| t != "entity" && t != "component")
                else {
                    continue;
                };
                // A component declaration repeats an entity, and the entity is the authority:
                // what an instance really connects to is the entity's ports. Where the two
                // disagree it is the component that is wrong, and `lint_750` says so.
                let ports = ports_of(&declaration, from_entity);
                match out.entry(name) {
                    std::collections::btree_map::Entry::Vacant(slot) => {
                        slot.insert(ports);
                    }
                    std::collections::btree_map::Entry::Occupied(mut slot) => {
                        if from_entity && !slot.get().from_entity {
                            slot.insert(ports);
                        }
                    }
                }
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
        // A statement whose unit cannot be read is still an instance, and its ports are still
        // connected to something. Skipping it would silently claim those signals have no driver;
        // an empty name reaches `undriven` as an entity it does not know, which marks everything
        // the instance touches as driven. Not knowing must never produce a finding.
        let entity = entity_of(&statement).unwrap_or_default();
        let mut associations = Vec::new();
        for map in find(&statement, NodeKind::PortMapAspect) {
            for association in find(&map, NodeKind::AssociationElement) {
                // The `Formal` node carries the `=>` with it.
                let formal = association
                    .children()
                    .find(|c| c.kind() == NodeKind::Formal)
                    .map(|formal| trimmed(text_of(&formal).trim_end_matches("=>")));
                let actual: String = association
                    .children()
                    .filter(|c| c.kind() != NodeKind::Formal)
                    .map(|c| text_of(&c))
                    .collect();
                associations.push((formal, trimmed(&actual)));
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
pub fn undriven(parsed: &Parsed, file: &Path, entities: &Entities) -> Vec<Finding> {
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
                    let text = lower(&token);
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
/// Component declarations in this file that do not match the entity they stand for.
///
/// A component declaration is a hand-written copy of an entity's interface, and the two drift:
/// a port is added to the entity and not to the component, or a mode is changed on one side.
/// What an instance binds to is decided by the component, so the design keeps elaborating and
/// means something other than it reads like -- until the day the binding fails instead.
///
/// Only entities the run can see are compared, so a component standing for a vendor primitive
/// is left alone.
#[must_use]
pub fn interfaces(parsed: &Parsed, file: &Path, entities: &Entities) -> Vec<Finding> {
    let mut findings = Vec::new();
    for declaration in find(parsed.root(), NodeKind::ComponentDeclaration) {
        let Some(name) = all_tokens(&declaration)
            .iter()
            .map(lower)
            .find(|t| t != "component")
        else {
            continue;
        };
        // Only against the entity itself: a table entry that came from another component
        // declaration says nothing about which of the two is right.
        let Some(entity) = entities.get(&name).filter(|p| p.from_entity) else {
            continue;
        };
        let component = ports_of(&declaration, false);

        let missing: Vec<&String> = entity
            .order
            .iter()
            .filter(|p| !component.order.contains(p))
            .collect();
        let extra: Vec<&String> = component
            .order
            .iter()
            .filter(|p| !entity.order.contains(p))
            .collect();
        let remoded: Vec<&String> = component
            .order
            .iter()
            .filter(|p| entity.order.contains(p))
            .filter(|p| {
                entity.driving.contains(*p) != component.driving.contains(*p)
                    || entity.reading.contains(*p) != component.reading.contains(*p)
            })
            .collect();
        // Order matters only for positional association, and differing order with the same names
        // is legal and common. It is reported because a positional instantiation then connects
        // the wrong signals, which is the worst of the three to debug.
        let reordered = missing.is_empty()
            && extra.is_empty()
            && remoded.is_empty()
            && entity.order != component.order;

        let mut faults = Vec::new();
        let list = |what: &str, ports: &[&String]| {
            format!(
                "{what} {}",
                ports
                    .iter()
                    .map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        if !missing.is_empty() {
            faults.push(list("does not declare", &missing));
        }
        if !extra.is_empty() {
            faults.push(list("declares, which the entity does not have,", &extra));
        }
        if !remoded.is_empty() {
            faults.push(list("gives a different mode to", &remoded));
        }
        if reordered {
            faults.push("declares its ports in a different order".to_owned());
        }
        if faults.is_empty() {
            continue;
        }
        let (line, column) = all_tokens(&declaration)
            .first()
            .map_or((1, 1), |token| parsed.line_col(token.text_offset()));
        findings.push(Finding {
            file: file.to_path_buf(),
            rule: "lint_750",
            line,
            column,
            message: format!(
                "Component '{name}' does not match entity '{name}': it {}. An instance binds to \
                 the component, so the two disagreeing means the design connects something other \
                 than it reads like.",
                faults.join("; and it ")
            ),
            related: Vec::new(),
        });
    }
    findings.sort_by_key(|f| (f.line, f.column));
    findings
}

pub const RULES: &[(&str, &str)] = &[
    (
        "lint_730",
        "A signal is read but nothing drives it: no assignment, and no instance output.",
    ),
    (
        "lint_750",
        "A component declaration does not match the entity it stands for.",
    ),
];

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

    /// The findings of `interfaces` for one file, against a project holding `entity`.
    fn interface_check(entity: &str, source: &str) -> Vec<String> {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("e.vhd");
        std::fs::write(&path, entity).expect("write");
        let known = entities(std::slice::from_ref(&path));
        let parsed = Parsed::new(source.as_bytes().to_vec());
        assert!(parsed.syntax_errors().is_empty(), "test source must parse");
        interfaces(&parsed, Path::new("top.vhd"), &known)
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    const DUT: &str = "entity dut is\n  port (\n    clk : in bit;\n    d : in bit;\n    \
                       q : out bit\n  );\nend entity dut;\n";

    /// An architecture whose declarative part holds `component`.
    fn top(component: &str) -> String {
        format!(
            "entity top is\nend entity top;\n\narchitecture rtl of top is\n{component}\
                 begin\nend architecture rtl;\n"
        )
    }

    #[test]
    fn a_component_that_matches_its_entity_is_silent() {
        let found = interface_check(
            DUT,
            &top(
                "  component dut is\n    port (\n      clk : in bit;\n      d : in bit;\n      \
                  q : out bit\n    );\n  end component dut;\n",
            ),
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_port_the_component_does_not_declare() {
        let found = interface_check(
            DUT,
            &top(
                "  component dut is\n    port (\n      clk : in bit;\n      d : in bit\n    \
                  );\n  end component dut;\n",
            ),
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("does not declare q"), "{found:?}");
    }

    #[test]
    fn a_port_the_entity_does_not_have_and_a_changed_mode() {
        let found = interface_check(
            DUT,
            &top(
                "  component dut is\n    port (\n      clk : in bit;\n      d : out bit;\n      \
                  q : out bit;\n      spare : in bit\n    );\n  end component dut;\n",
            ),
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("does not have, spare"), "{found:?}");
        assert!(found[0].contains("different mode to d"), "{found:?}");
    }

    #[test]
    fn the_same_ports_in_a_different_order() {
        // Legal, and harmless until someone instantiates positionally.
        let found = interface_check(
            DUT,
            &top(
                "  component dut is\n    port (\n      d : in bit;\n      clk : in bit;\n      \
                  q : out bit\n    );\n  end component dut;\n",
            ),
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("different order"), "{found:?}");
    }

    #[test]
    fn a_component_for_an_entity_the_run_cannot_see_is_left_alone() {
        let found = interface_check(
            "entity other is\nend entity other;\n",
            &top(
                "  component vendor_pll is\n    port (\n      refclk : in bit\n    );\n  \
                  end component vendor_pll;\n",
            ),
        );
        assert!(found.is_empty(), "{found:?}");
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
    fn a_component_instantiation_drives_its_actuals() {
        // `u : component driver` is the same instance as `u : entity work.driver`, but the
        // parser gives it a different node. Reading only the entity form meant the unit was
        // never identified, the instance was skipped, and every signal it drove was reported as
        // having no driver.
        let source = "entity tb is\nend entity tb;\n\narchitecture tb of tb is\n\n  \
                      signal reset : bit;\n\n  component driver is\n    port (\n      \
                      reset : out bit\n    );\n  end component;\n\nbegin\n\n  \
                      u : component driver\n    port map (\n      reset => reset\n    );\n\n  \
                      p : process (reset) is\n  begin\n    report bit'image(reset);\n  \
                      end process p;\n\nend architecture tb;\n";
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tb.vhd");
        std::fs::write(&path, source).expect("write");
        let entities = entities(std::slice::from_ref(&path));
        let parsed = Parsed::new(source.as_bytes().to_vec());
        let found = undriven(&parsed, &path, &entities);
        assert!(found.is_empty(), "the component drives reset: {found:?}");
    }

    #[test]
    fn an_instance_of_something_unreadable_drives_everything_it_touches() {
        // Whatever this is, it is connected to `spare`. Not knowing what must not become a claim
        // that nothing drives it.
        let source = "entity tb is\nend entity tb;\n\narchitecture tb of tb is\n\n  \
                      signal spare : bit;\n\nbegin\n\n  u : entity work.nowhere\n    \
                      port map (\n      q => spare\n    );\n\n  p : process (spare) is\n  \
                      begin\n    report bit'image(spare);\n  end process p;\n\n\
                      end architecture tb;\n";
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tb.vhd");
        std::fs::write(&path, source).expect("write");
        let parsed = Parsed::new(source.as_bytes().to_vec());
        let found = undriven(&parsed, &path, &Entities::new());
        assert!(
            found.is_empty(),
            "an unknown entity drives what it touches: {found:?}"
        );
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

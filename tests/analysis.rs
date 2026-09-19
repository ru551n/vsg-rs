//! The analysis layer, used as a library.
//!
//! This is the point of it living in `vsg_rs` rather than in the binary: a consumer that is not
//! the command line -- an editor server, another tool, a test -- can analyse source without
//! spawning a process and reading its report.

use std::path::Path;

use vsg_rs::Parsed;
use vsg_rs::analysis::{combinational, design, lint, width};

fn parse(source: &str) -> Parsed {
    let parsed = Parsed::new(source.as_bytes().to_vec());
    assert!(parsed.syntax_errors().is_empty(), "test source must parse");
    parsed
}

const TWO_DRIVERS: &str = "entity dut is\n  port (\n    a : in  bit;\n    b : in  bit;\n    \
                           q : out bit\n  );\nend entity dut;\n\n\
                           architecture rtl of dut is\n\nbegin\n\n  q <= a;\n  q <= b;\n\n\
                           end architecture rtl;\n";

#[test]
fn analysis_runs_without_the_command_line() {
    let naming = design::Naming {
        prefixes: Vec::new(),
        suffixes: Vec::new(),
    };
    let found = design::check(&parse(TWO_DRIVERS), Path::new("dut.vhd"), &naming);
    let drivers: Vec<_> = found.iter().filter(|f| f.rule == "lint_601").collect();
    assert_eq!(drivers.len(), 1, "{found:?}");
    // The finding carries its other locations as data, not as text.
    assert_eq!(drivers[0].related.len(), 2);
}

#[test]
fn findings_come_from_bytes_not_from_a_path() {
    // Nothing is written to disk: the source exists only here, and the path is a label.
    let source = "entity dut is\n  port (\n    wide   : in  bit_vector(15 downto 0);\n    \
                  narrow : out bit_vector(7 downto 0)\n  );\nend entity dut;\n\n\
                  architecture rtl of dut is\n\nbegin\n\n  narrow <= wide;\n\n\
                  end architecture rtl;\n";
    let found = width::check(&parse(source), Path::new("unsaved-buffer.vhd"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].rule, "lint_740");
    assert_eq!(found[0].file, Path::new("unsaved-buffer.vhd"));
}

#[test]
fn each_rule_module_is_reachable() {
    let source = "entity dut is\n  port (\n    a : in  bit;\n    y : out bit\n  );\n\
                  end entity dut;\n\narchitecture rtl of dut is\n\n  signal grant   : bit;\n  \
                  signal request : bit;\n\nbegin\n\n  grant   <= request and a;\n  \
                  request <= grant or a;\n  y       <= grant;\n\nend architecture rtl;\n";
    let found = combinational::check(&parse(source), Path::new("dut.vhd"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].rule, "lint_720");
}

#[test]
fn a_source_is_a_path_or_a_buffer_standing_in_for_one() {
    // What an editor holds, for a file that may never have been saved.
    let buffer = lint::Source::buffer("unsaved.vhd", b"entity e is\nend entity e;\n".to_vec());
    assert_eq!(buffer.path, Path::new("unsaved.vhd"));
    assert!(buffer.text.is_some());

    // What the command line names.
    let file = lint::Source::file("on-disk.vhd");
    assert!(file.text.is_none(), "a file is read when it is analysed");
}

//! Telling testbench code from synthesisable code, so that each can be given its own rules.
//!
//! A latch or a second driver means nothing in a testbench: it drives a signal from two places
//! on purpose and holds values between `wait`s by design. Rules about *code* — unused
//! declarations, sensitivity lists, types — still apply. Rather than guess which is which,
//! vsg-rs classifies the file and applies the `rtl:` or `testbench:` rule block from the
//! configuration; what those blocks contain is the project's decision.
//!
//! Three ways to decide, most authoritative first:
//!
//! 1. `vsg_rs: testbench_files` — globs, like Linty's simulation paths.
//! 2. `-- vsg-rs: testbench` near the top of a file, for the one file no glob covers.
//! 3. Otherwise the shape of the code. Measured over three corpora: 0 of 18 real RTL files were
//!    classified as testbench, and 16 of 16 testbenches were.

use std::path::Path;

use vhdl_syntax::syntax::{NodeKind, SyntaxNode};
use vsg_rs::Parsed;

/// Match a path against one `testbench_files` pattern. A pattern with no `/` is about the file
/// name wherever it lives (`tb_*.vhd`), one with a `/` is about the path (`test/**`, and a
/// relative pattern also matches deeper, so `test/**` covers `modules/fifo/test/tb.vhd`).
fn matches(pattern: &str, path: &str) -> bool {
    let glob = |p: &str, t: &str| vsg_rs::config::glob(p.as_bytes(), t.as_bytes());
    if !pattern.contains('/') {
        let name = path.rsplit('/').next().unwrap_or(path);
        return glob(pattern, name);
    }
    glob(pattern, path) || glob(&format!("**/{}", pattern.trim_start_matches("./")), path)
}

/// Directories and file names that say "testbench" by convention.
fn named_like_a_testbench(path: &Path) -> bool {
    let text = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let name = text.rsplit('/').next().unwrap_or(&text);
    let stem = name.split('.').next().unwrap_or(name);
    stem.starts_with("tb_")
        || stem.ends_with("_tb")
        || text.split('/').any(|part| {
            matches!(
                part,
                "test"
                    | "tests"
                    | "tb"
                    | "testbench"
                    | "testbenches"
                    | "sim"
                    | "bench"
                    | "verification"
            )
        })
}

fn nodes(node: &SyntaxNode, kind: NodeKind, out: &mut Vec<SyntaxNode>) {
    for child in node.children() {
        if child.kind() == kind {
            out.push(child.clone());
        }
        nodes(&child, kind, out);
    }
}

fn find(node: &SyntaxNode, kind: NodeKind) -> Vec<SyntaxNode> {
    let mut out = Vec::new();
    nodes(node, kind, &mut out);
    out
}

/// A testbench top has nothing to connect to: no ports, only generics if anything.
fn has_an_entity_without_ports(root: &SyntaxNode) -> bool {
    let entities = find(root, NodeKind::EntityDeclaration);
    !entities.is_empty()
        && entities
            .iter()
            .any(|e| find(e, NodeKind::PortClause).is_empty())
}

/// Why a file was treated as a testbench, or `None` when it is synthesisable code.
pub(crate) fn classify(parsed: &Parsed, path: &Path, patterns: &[String]) -> Option<&'static str> {
    let name = path.to_string_lossy().replace('\\', "/");
    let name = name.trim_start_matches("./").to_owned();
    if patterns.iter().any(|pattern| matches(pattern, &name)) {
        return Some("listed in `vsg_rs: testbench_files`");
    }
    // The directive, near the top of the file rather than buried in the middle of it.
    let head = &parsed.source()[..parsed.source().len().min(2000)];
    let head = String::from_utf8_lossy(head).to_ascii_lowercase();
    if head.contains("vsg-rs: testbench") {
        return Some("marked with `-- vsg-rs: testbench`");
    }
    // A verification framework in the context clause, or VUnit's own generic.
    let code = code_of(parsed.root());
    if ["vunit_lib", "osvvm", "uvvm_util", "runner_cfg", "bitvis"]
        .iter()
        .any(|library| code.contains(library))
    {
        return Some("uses a verification library");
    }
    if has_an_entity_without_ports(parsed.root()) {
        return Some("declares an entity with no ports");
    }
    if named_like_a_testbench(path) {
        return Some("named like a testbench");
    }
    None
}

/// The code of a node, lowercased and without comments: a comment mentioning OSVVM should not
/// turn a design file into a testbench.
fn code_of(node: &SyntaxNode) -> String {
    let mut out = String::new();
    let mut stack = vec![node.clone()];
    while let Some(node) = stack.pop() {
        for element in node.children_with_tokens() {
            match element {
                vhdl_syntax::syntax::SyntaxElement::Node(child) => stack.push(child),
                vhdl_syntax::syntax::SyntaxElement::Token(t) => {
                    out += &String::from_utf8_lossy(t.text().as_bytes()).to_ascii_lowercase();
                    out.push(' ');
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classified(source: &str, path: &str, patterns: &[&str]) -> Option<&'static str> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        let patterns: Vec<String> = patterns.iter().map(|p| (*p).to_owned()).collect();
        classify(&parsed, Path::new(path), &patterns)
    }

    const RTL: &str = "entity adder is\n  port (\n    a : in bit;\n    y : out bit\n  );\n\
                       end entity adder;\n\narchitecture rtl of adder is\nbegin\n  y <= a;\n\
                       end architecture rtl;\n";

    #[test]
    fn real_rtl_is_not_a_testbench() {
        assert_eq!(classified(RTL, "src/adder.vhd", &[]), None);
    }

    #[test]
    fn an_entity_without_ports_is_a_testbench() {
        let source = "entity tb is\nend entity tb;\n\narchitecture sim of tb is\nbegin\n\
                      end architecture sim;\n";
        assert!(classified(source, "src/thing.vhd", &[]).is_some());
    }

    #[test]
    fn a_verification_library_is_a_testbench() {
        let source = format!("library vunit_lib;\n  use vunit_lib.run_pkg.all;\n\n{RTL}");
        assert_eq!(
            classified(&source, "src/adder.vhd", &[]),
            Some("uses a verification library")
        );
    }

    #[test]
    fn names_and_directories_count() {
        assert!(classified(RTL, "test/adder.vhd", &[]).is_some());
        assert!(classified(RTL, "src/tb_adder.vhd", &[]).is_some());
        assert!(classified(RTL, "src/adder_tb.vhd", &[]).is_some());
        assert_eq!(classified(RTL, "src/testbed_helper.vhd", &[]), None);
    }

    #[test]
    fn glob_patterns() {
        // A bare name is about the file wherever it is.
        assert!(matches("tb_*.vhd", "modules/fifo/test/tb_fifo.vhd"));
        assert!(!matches("tb_*.vhd", "modules/fifo/src/fifo.vhd"));
        // A path pattern matches from the root, and from any directory below it.
        assert!(matches("test/**", "test/tb_fifo.vhd"));
        assert!(matches("test/**", "modules/fifo/test/tb_fifo.vhd"));
        assert!(!matches("test/**", "modules/fifo/src/fifo.vhd"));
        assert!(matches("**/verify/*.vhd", "a/b/verify/thing.vhd"));
        assert!(matches("sim/*.vhd", "sim/harness.vhd"));
        assert!(!matches("sim/*.vhd", "sim/deep/harness.vhd"));
    }

    #[test]
    fn configuration_wins() {
        assert_eq!(
            classified(RTL, "verify/adder.vhd", &["verify/**"]),
            Some("listed in `vsg_rs: testbench_files`")
        );
    }

    #[test]
    fn the_directive_marks_one_file() {
        let source = format!("-- vsg-rs: testbench\n{RTL}");
        assert_eq!(
            classified(&source, "src/adder.vhd", &[]),
            Some("marked with `-- vsg-rs: testbench`")
        );
    }
}

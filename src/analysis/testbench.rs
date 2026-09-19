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
//! 1. `vsg_rs: testbench_files` — globs naming the testbenches.
//! 2. `vsg_rs: testbench_libraries` — library names from the project's `vhdl_ls.toml`, for
//!    projects that already say which library each file belongs to.
//! 3. `-- vsg-rs: testbench` near the top of a file, for the one file no glob covers.
//! 4. Otherwise the shape of the code. Measured over three corpora: 0 of 18 real RTL files were
//!    classified as testbench, and 16 of 16 testbenches were.

use std::path::Path;

use crate::Parsed;
use vhdl_syntax::syntax::{NodeKind, SyntaxNode};

use super::design::{find, text_of};

/// Match a path against one `testbench_files` pattern. A pattern with no `/` is about the file
/// name wherever it lives (`tb_*.vhd`), one with a `/` is about the path (`test/**`, and a
/// relative pattern also matches deeper, so `test/**` covers `modules/fifo/test/tb.vhd`).
fn matches(pattern: &str, path: &str) -> bool {
    let glob = |p: &str, t: &str| crate::config::glob(p.as_bytes(), t.as_bytes());
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

/// A testbench top has nothing to connect to: no ports, only generics if anything.
fn has_an_entity_without_ports(root: &SyntaxNode) -> bool {
    let entities = find(root, NodeKind::EntityDeclaration);
    !entities.is_empty()
        && entities
            .iter()
            .any(|e| find(e, NodeKind::PortClause).is_empty())
}

/// What the run knows about which files are testbenches.
pub struct Kinds<'a> {
    /// `vsg_rs: testbench_files`.
    pub patterns: &'a [String],
    /// `vsg_rs: testbench_libraries`, lowercased.
    pub libraries: &'a [String],
    /// The libraries each file belongs to, from `vhdl_ls.toml`.
    pub of_file: &'a std::collections::BTreeMap<std::path::PathBuf, Vec<String>>,
}

/// Why a file was treated as a testbench, or `None` when it is synthesisable code.
pub fn classify(parsed: &Parsed, path: &Path, kinds: &Kinds<'_>) -> Option<&'static str> {
    let name = path.to_string_lossy().replace('\\', "/");
    let name = name.trim_start_matches("./").to_owned();
    if kinds.patterns.iter().any(|pattern| matches(pattern, &name)) {
        return Some("listed in `vsg_rs: testbench_files`");
    }
    if !kinds.libraries.is_empty() {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let belongs_to = kinds
            .of_file
            .get(&canonical)
            .or_else(|| kinds.of_file.get(path));
        if belongs_to.is_some_and(|libraries| {
            libraries
                .iter()
                .any(|library| kinds.libraries.iter().any(|test| test == library))
        }) {
            return Some("in a library listed in `vsg_rs: testbench_libraries`");
        }
    }
    // The directive, near the top of the file rather than buried in the middle of it.
    let head = &parsed.source()[..parsed.source().len().min(2000)];
    let head = String::from_utf8_lossy(head).to_ascii_lowercase();
    if head.contains("vsg-rs: testbench") {
        return Some("marked with `-- vsg-rs: testbench`");
    }
    // A verification framework in the context clause, or VUnit's own generic. Comments are not
    // tokens, so a comment mentioning OSVVM does not turn a design file into a testbench.
    let code = text_of(parsed.root());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn classified(source: &str, path: &str, patterns: &[&str]) -> Option<&'static str> {
        let parsed = Parsed::new(source.as_bytes().to_vec());
        let patterns: Vec<String> = patterns.iter().map(|p| (*p).to_owned()).collect();
        let of_file = std::collections::BTreeMap::new();
        classify(
            &parsed,
            Path::new(path),
            &Kinds {
                patterns: &patterns,
                libraries: &[],
                of_file: &of_file,
            },
        )
    }

    #[test]
    fn a_test_library_marks_its_files() {
        let parsed = Parsed::new(RTL.as_bytes().to_vec());
        let path = Path::new("src/adder.vhd");
        let mut of_file = std::collections::BTreeMap::new();
        of_file.insert(path.to_path_buf(), vec!["tb_lib".to_owned()]);
        let kinds = Kinds {
            patterns: &[],
            libraries: &["tb_lib".to_owned()],
            of_file: &of_file,
        };
        assert_eq!(
            classify(&parsed, path, &kinds),
            Some("in a library listed in `vsg_rs: testbench_libraries`")
        );
        let kinds = Kinds {
            patterns: &[],
            libraries: &["other_lib".to_owned()],
            of_file: &of_file,
        };
        assert_eq!(classify(&parsed, path, &kinds), None);
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

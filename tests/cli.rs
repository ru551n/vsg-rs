//! End-to-end tests of the `vsg-rs` command line.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const UNFORMATTED: &str = "entity e is port (a : in bit); end;\n";
const FORMATTED: &str = "entity e is\n  port (\n    a : in    bit\n  );\nend entity e;\n";
const MALFORMED: &str = "entity e is port (a : in bit; end;\n";

fn vsg(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vsg-rs"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn vsg-rs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait")
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write file");
    path
}

#[test]
fn stdin_fix_prints_only_source() {
    let out = vsg(&["--stdin", "--fix"], UNFORMATTED);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), FORMATTED);
}

#[test]
fn stdin_fix_succeeds_with_remaining_violations() {
    // process_016 (missing process label) has no fix.
    let src = "architecture a of e is\nbegin\n  process is\n  begin\n    wait;\n  end process;\nend architecture a;\n";
    let out = vsg(&["--stdin", "--fix"], src);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("process_016"));
}

#[test]
fn syntax_errors_are_reported_and_nothing_is_changed() {
    let out = vsg(&["--stdin", "--fix"], MALFORMED);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("syntax"), "{err}");
    let dir = tempfile::tempdir().expect("tempdir");
    let bad = write(dir.path(), "bad.vhd", MALFORMED);
    let good = write(dir.path(), "good.vhd", UNFORMATTED);
    let out = vsg(
        &["-f", bad.to_str().unwrap(), good.to_str().unwrap(), "--fix"],
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(bad).unwrap(), MALFORMED);
    // Other files are still processed.
    assert_eq!(std::fs::read_to_string(good).unwrap(), FORMATTED);
}

#[test]
fn fix_in_place_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(dir.path(), "a.vhd", UNFORMATTED);
    let path = file.to_str().unwrap();
    let report = vsg(&[path], "");
    assert_eq!(report.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), UNFORMATTED);
    assert!(vsg(&[path, "--fix"], "").status.success());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), FORMATTED);
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    assert!(vsg(&[path], "").status.success());
    assert!(vsg(&[path, "--fix"], "").status.success());
    // Unchanged output is not rewritten.
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        modified
    );
}

#[test]
fn crlf_is_preserved() {
    let out = vsg(&["--stdin", "--fix"], &UNFORMATTED.replace('\n', "\r\n"));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        FORMATTED.replace('\n', "\r\n")
    );
}

#[test]
fn comment_only_input() {
    let out = vsg(&["--stdin", "--fix"], "\n\n-- only a comment   \n\n");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-- only a comment\n");
}

#[test]
fn line_length_from_configuration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = write(
        dir.path(),
        "c.yaml",
        "rule:\n  length_001:\n    length: 20\n",
    );
    let src = "architecture a of e is begin x <= f(alpha, beta, gamma); end architecture a;\n";
    let out = vsg(&["--stdin", "--fix", "-c", cfg.to_str().unwrap()], src);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("  x <= f(\n    alpha,\n"), "{text}");
}

#[test]
fn unsafe_fixes_are_opt_in() {
    let src = "entity e is\n  port (a : bit);\nend entity e;\n";
    let safe = vsg(&["--stdin", "--fix"], src);
    assert!(String::from_utf8_lossy(&safe.stdout).contains("a : bit"));
    let all = vsg(&["--stdin", "--fix", "--unsafe_fixes"], src);
    assert!(
        String::from_utf8_lossy(&all.stdout).contains("a : in    bit"),
        "{all:?}"
    );
}

// ------------------------------------------------------------------ VSG command line

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vsg-rs-cli-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create dir");
    dir
}

#[test]
fn vsg_style_report_and_exit_code() {
    let dir = scratch("report");
    let bad = write(&dir, "a.vhd", "entity e is\nend;\n");
    let good = write(&dir, "b.vhd", "entity e is\nend entity e;\n");
    let out = vsg(&["-f", bad.to_str().unwrap(), good.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Phase 1 of 1... Reporting"), "{text}");
    assert!(
        text.contains("  entity_015 | Error      |          2 | Add *entity* keyword"),
        "{text}"
    );
    assert!(text.contains("Total Violations:    0"), "{text}");
    let good_only = vsg(&[good.to_str().unwrap(), "-of", "summary"], "");
    assert_eq!(good_only.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&good_only.stdout).contains(" OK ("));
}

#[test]
fn vsg_style_fix_with_backup_and_reports() {
    let dir = scratch("fix");
    let file = write(&dir, "a.vhd", "entity e is\nend;\n");
    let json = dir.join("out.json");
    let junit = dir.join("out.xml");
    let out = vsg(
        &[
            "-f",
            file.to_str().unwrap(),
            "--fix",
            "-b",
            "-js",
            json.to_str().unwrap(),
            "-j",
            junit.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "entity e is\nend entity e;\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("a.vhd.bak")).unwrap(),
        "entity e is\nend;\n"
    );
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(report["files"][0]["violations"], serde_json::json!([]));
    assert!(
        std::fs::read_to_string(&junit)
            .unwrap()
            .contains("tests=\"1\"")
    );
}

#[test]
fn vsg_style_stdin_and_rule_configuration() {
    let out = vsg(&["--stdin", "-of", "syntastic"], "entity e is\nend;\n");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "ERROR: stdin(2)entity_015 -- Add *entity* keyword\n\
         ERROR: stdin(2)entity_019 -- Add entity simple name\n"
    );
    let fixed = vsg(&["--stdin", "--fix"], "entity e is\nend;\n");
    assert_eq!(
        String::from_utf8_lossy(&fixed.stdout),
        "entity e is\nend entity e;\n"
    );
    let rc = vsg(&["-rc", "entity_015"], "");
    let value: serde_json::Value = serde_json::from_slice(&rc.stdout).unwrap();
    assert_eq!(value["rule"]["entity_015"]["action"], "add");
    let missing = vsg(&["-f", "does/not/exist.vhd"], "");
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("does not exist"));
}

#[test]
fn extensions_diff_range_sarif_and_rule_list() {
    let diff = vsg(&["--stdin", "--fix", "--diff"], UNFORMATTED);
    assert!(String::from_utf8_lossy(&diff.stdout).contains("+  port ("));
    let src = "entity e is\nend entity e;\narchitecture rtl of e is\nbegin\n  a<=b;\n  c<=d;\nend architecture rtl;\n";
    let range = vsg(&["--stdin", "--fix", "--range", "6:6"], src);
    assert_eq!(
        String::from_utf8_lossy(&range.stdout),
        src.replace("c<=d", "c <= d")
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let sarif = dir.path().join("out.sarif");
    let out = vsg(
        &["--stdin", "--sarif", sarif.to_str().unwrap()],
        "entity e is\nend;\n",
    );
    assert_eq!(out.status.code(), Some(1));
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&sarif).unwrap()).unwrap();
    assert_eq!(doc["runs"][0]["results"][0]["ruleId"], "entity_015");
    let list = vsg(&["--list_rules"], "");
    let listed = String::from_utf8_lossy(&list.stdout);
    // Every VSG rule, and the lint layer's own rules after them.
    assert_eq!(
        listed.lines().filter(|l| !l.starts_with("lint_")).count(),
        972
    );
    assert!(
        listed.lines().any(|l| l.starts_with("lint_001")),
        "lint rules are listed"
    );
}

#[test]
fn sarif_carries_related_locations_and_safe_fixes() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Two drivers of one signal: a finding about more than one place.
    let file = write(
        dir.path(),
        "dut.vhd",
        "entity dut is\n  port (\n    a : in  bit;\n    b : in  bit;\n    q : out bit\n  );\n\
         end;\n\narchitecture rtl of dut is\n\nbegin\n\n  q <= a;\n  q <= b;\n\n\
         end architecture rtl;\n",
    );
    let sarif = dir.path().join("out.sarif");
    let out = vsg(
        &[
            file.to_str().unwrap(),
            "--check",
            "style,lint",
            "--sarif",
            sarif.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&sarif).unwrap()).unwrap();
    let results = doc["runs"][0]["results"].as_array().expect("results");

    // The drivers are locations, not a sentence.
    let drivers = results
        .iter()
        .find(|r| r["ruleId"] == "lint_601")
        .expect("lint_601 reported");
    let related = drivers["relatedLocations"]
        .as_array()
        .expect("relatedLocations");
    assert_eq!(related.len(), 2, "{related:?}");
    for one in related {
        assert!(
            one["physicalLocation"]["region"]["startLine"]
                .as_u64()
                .unwrap_or(0)
                >= 1
        );
        assert!(one["message"]["text"].as_str().is_some());
    }

    // A safe fix travels as an edit a consumer can apply, with a region and its text.
    let fixed = results
        .iter()
        .find(|r| r.get("fixes").is_some())
        .expect("some result carries a fix");
    let replacement = &fixed["fixes"][0]["artifactChanges"][0]["replacements"][0];
    assert!(
        replacement["deletedRegion"]["startLine"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );
    assert!(replacement["insertedContent"]["text"].as_str().is_some());

    // A lint finding never carries one, because applying it would change the design.
    assert!(drivers.get("fixes").is_none(), "{drivers:?}");
}

#[test]
fn sonarqube_report_carries_the_layer_as_the_issue_type() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(
        dir.path(),
        "dut.vhd",
        "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  signal a, b, c : bit;\n\
         \n         begin\n\n  p : process (a, b) is\n  begin\n    if a = '1' then\n      \
         c <= b;\n    end if;\n  end process p;\n\nend architecture rtl;\n",
    );
    let report = dir.path().join("sonar.json");
    let out = vsg(
        &[
            file.to_str().unwrap(),
            "--check",
            "style,lint",
            "--sonarqube",
            report.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(out.status.code(), Some(1));
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report).unwrap()).unwrap();
    let issues = doc["issues"].as_array().expect("an issues array");
    assert!(!issues.is_empty());
    for issue in issues {
        assert_eq!(issue["engineId"], "vsg-rs");
        let rule = issue["ruleId"].as_str().unwrap_or_default();
        // The layer decides the type: a lint finding is a bug, style is a code smell.
        let expected = if rule.starts_with("lint_") {
            "BUG"
        } else {
            "CODE_SMELL"
        };
        assert_eq!(issue["type"], expected, "{rule}");
        let severity = issue["severity"].as_str().unwrap_or_default();
        assert!(
            ["INFO", "MINOR", "MAJOR", "CRITICAL"].contains(&severity),
            "{rule}: {severity}"
        );
        // A lint finding never carries a fix, so it is never filed as trivially fixable.
        if rule.starts_with("lint_") {
            assert_ne!(severity, "INFO", "{rule}");
        }
        let range = &issue["primaryLocation"]["textRange"];
        assert!(range["startLine"].as_u64().unwrap_or_default() >= 1);
        // SonarQube counts columns from zero, so the report never carries a negative one.
        assert!(range["startColumn"].as_i64().unwrap_or_default() >= 0);
    }
    // The latch this file infers is reported, as a bug, and as something to act on.
    assert!(
        issues.iter().any(|i| i["ruleId"] == "lint_600"
            && i["type"] == "BUG"
            && i["severity"] == "CRITICAL"),
        "the lint layer reaches the report"
    );
    // The misplaced `begin` is layout, which --fix repairs, so it is filed as informational.
    assert!(
        issues.iter().any(|i| i["severity"] == "INFO"),
        "a fixable finding is not technical debt"
    );
}

#[test]
fn lint_is_a_subcommand_and_the_root_stays_vsg() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(
        dir.path(),
        "dut.vhd",
        "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  signal a, b, c : bit;\n\n         begin\n\n  p : process (a, b) is\n  begin\n    if a = '1' then\n      c <= b;\n    \
         end if;\n  end process p;\n\nend architecture rtl;\n",
    );
    let path = file.to_str().unwrap();
    // The root command is VSG's, and reports VSG's rules.
    let bare = vsg(&[path, "--output_format", "syntastic"], "");
    assert!(String::from_utf8_lossy(&bare.stdout).contains("process_"));
    // `lint` reports its own rules and none of VSG's.
    let lint = vsg(&["lint", path, "--output_format", "syntastic"], "");
    let out = String::from_utf8_lossy(&lint.stdout);
    assert!(out.contains("lint_600"), "{out}");
    assert!(!out.contains("process_"), "only the lint layer runs: {out}");
    // Both layers, still one command.
    let both = vsg(
        &[
            "lint",
            path,
            "--check",
            "style,lint",
            "--output_format",
            "syntastic",
        ],
        "",
    );
    let out = String::from_utf8_lossy(&both.stdout);
    assert!(
        out.contains("lint_600") && out.contains("process_"),
        "{out}"
    );
    // A file of that name still wins over the subcommand.
    let named = dir.path().join("lint");
    std::fs::copy(&file, &named).expect("copy");
    let by_name = vsg(
        &[named.to_str().unwrap(), "--output_format", "syntastic"],
        "",
    );
    assert!(
        String::from_utf8_lossy(&by_name.stdout).contains("process_"),
        "a file named `lint` is a file"
    );
}

#[test]
fn layers_can_be_gated_and_explained() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(
        dir.path(),
        "dut.vhd",
        "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  signal a, b, c : bit;\n\n\
         begin\n\n  p : process (a, b) is\n  begin\n    if a = '1' then\n      c <= b;\n    \
         end if;\n  end process p;\n\nend architecture rtl;\n",
    );
    let path = file.to_str().unwrap();
    // The lint layer fails the run when gated on it, the style layer does not.
    let on_lint = vsg(&["lint", path, "--fail_on", "lint"], "");
    assert_eq!(on_lint.status.code(), Some(1));
    let on_style = vsg(&["lint", path, "--fail_on", "style"], "");
    assert_eq!(
        on_style.status.code(),
        Some(0),
        "no style findings to gate on"
    );
    // An unknown layer is refused rather than ignored.
    let bad = vsg(&[path, "--fail_on", "nonsense"], "");
    assert_eq!(bad.status.code(), Some(1));
    // --statistics names the layer of each rule.
    let stats = vsg(&["lint", path, "--statistics"], "");
    let out = String::from_utf8_lossy(&stats.stdout);
    assert!(out.contains("lint_600"), "{out}");
    assert!(out.contains("lint"), "{out}");
    // --explain describes a rule from either layer, and refuses an unknown one.
    for rule in ["lint_600", "entity_019"] {
        let explained = vsg(&["--explain", rule], "");
        assert_eq!(explained.status.code(), Some(0), "{rule}");
        assert!(
            String::from_utf8_lossy(&explained.stdout).contains("Layer:"),
            "{rule}"
        );
    }
    assert_eq!(vsg(&["--explain", "nope_001"], "").status.code(), Some(1));
}

#[test]
fn configuration_is_discovered_next_to_the_input() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "vsg-rs.yaml",
        "rule:\n  entity_015:\n    disable: true\n",
    );
    let file = write(dir.path(), "a.vhd", "entity e is\nend e;\n");
    let out = vsg(&[file.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

#[test]
fn file_list_and_recursive_directories() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("src/sub")).unwrap();
    let a = write(dir.path(), "src/a.vhd", "entity A is\nend entity A;\n");
    write(dir.path(), "src/sub/b.vhd", "entity B is\nend entity B;\n");
    let root = dir.path().to_str().unwrap();
    let cfg = write(
        dir.path(),
        "c.yaml",
        &format!(
            "file_list:\n  - '{root}/src/a.vhd'\n  - '{root}/src/**/b.vhd':\n      rule:\n        entity_008:\n          case: upper\n"
        ),
    );
    let out = vsg(&["-c", cfg.to_str().unwrap(), "-of", "syntastic"], "");
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.matches("entity_008").count(), 1, "{text}");
    assert!(text.contains("a.vhd(1)entity_008"), "{text}");
    // Files named on the command line and in the list are checked once.
    let both = vsg(
        &[
            "-c",
            cfg.to_str().unwrap(),
            "-f",
            a.to_str().unwrap(),
            "-of",
            "summary",
        ],
        "",
    );
    assert_eq!(
        String::from_utf8_lossy(&both.stdout)
            .matches("File: ")
            .count(),
        2,
        "{both:?}"
    );
    let missing = write(dir.path(), "m.yaml", "file_list:\n  - nothing/*.vhd\n");
    let out = vsg(&["-c", missing.to_str().unwrap()], "");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Could not find file nothing/*.vhd"));
    // Directories are only searched with --recursive.
    let src = dir.path().join("src");
    let flat = vsg(&[src.to_str().unwrap(), "-of", "syntastic"], "");
    assert!(String::from_utf8_lossy(&flat.stdout).contains("source_file_001"));
    let deep = vsg(
        &["--recursive", src.to_str().unwrap(), "-of", "syntastic"],
        "",
    );
    assert_eq!(
        String::from_utf8_lossy(&deep.stdout)
            .matches("entity_008")
            .count(),
        2
    );
}

#[test]
fn worker_processes_match_one_process() {
    // Enough input for several worker processes.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut files = vec![write(
        dir.path(),
        "pkg.vhd",
        "package pkg is\n  constant C_Width : natural := 8;\nend package pkg;\n",
    )];
    let body = "  x <= C_WIDTH;\n".repeat(4000);
    for i in 0..4 {
        files.push(write(
            dir.path(),
            &format!("a{i}.vhd"),
            &format!(
                "use work.pkg.all;\narchitecture a of e{i} is\nbegin\n{body}end architecture a;\n"
            ),
        ));
    }
    let names: Vec<&str> = files.iter().map(|f| f.to_str().unwrap()).collect();
    let run = |jobs: &str| {
        let mut args = vec!["--debug", "-p", jobs, "-of", "syntastic", "-f"];
        args.extend(&names);
        vsg(&args, "")
    };
    let (one, many) = (run("1"), run("4"));
    assert_eq!(one.stdout, many.stdout);
    let debug = String::from_utf8_lossy(&many.stderr);
    assert!(
        debug.contains("process(es)") && !debug.contains("by 1 process"),
        "{debug}"
    );
    // The cross-file consistency check works in workers too.
    assert!(String::from_utf8_lossy(&many.stdout).contains("a3.vhd(5)constant_013"));
    let mut args = vec!["-p", "4", "--fix", "-f"];
    args.extend(&names);
    vsg(&args, "");
    let fixed = std::fs::read_to_string(&files[4]).unwrap();
    assert!(!fixed.contains("C_WIDTH"), "{}", &fixed[..200]);
}

#[test]
fn local_rules_run_through_vsg() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("rules")).unwrap();
    let file = write(dir.path(), "a.vhd", "entity e is\nend entity e; -- TODO\n");
    let python = if cfg!(windows) { "python" } else { "python3" };
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_vsg.py");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_vsg-rs"))
            .args(args)
            .env("VSG_RS_VSG", format!("{python} {}", fake.display()))
            .current_dir(dir.path())
            .output()
            .expect("run vsg-rs")
    };
    let check = run(&["-lr", "rules", "-of", "syntastic", "-f", "a.vhd"]);
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert_eq!(
        String::from_utf8_lossy(&check.stdout),
        "ERROR: a.vhd(2)fake_001 -- Replace TODO\n"
    );
    // The configuration's local_rules works too, and --fix applies the local fixes.
    write(
        dir.path(),
        "c.yaml",
        "local_rules: rules\nrule:\n  fake_001:\n    disable: false\n",
    );
    let fix = run(&["-c", "c.yaml", "--fix", "-f", "a.vhd"]);
    assert!(fix.status.success(), "{fix:?}");
    assert!(
        !String::from_utf8_lossy(&fix.stderr).contains("WARNING"),
        "{fix:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "entity e is\nend entity e; -- DONE\n"
    );
    let missing = run(&["-lr", "nowhere", "-f", "a.vhd"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("does not exist"));
}

#[test]
fn statistics_counts_violations_per_rule() {
    let dir = tempfile::tempdir().expect("tempdir");
    let a = write(dir.path(), "a.vhd", "entity E is\nend;\n");
    let b = write(dir.path(), "b.vhd", "entity F is\nend;\n");
    let out = vsg(
        &[
            "--statistics",
            "-of",
            "summary",
            "-f",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
        ],
        "",
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("rule"), "{text}");
    // entity_008 (name case) is reported once per file, and its fixes are on by default.
    let line = text
        .lines()
        .find(|l| l.starts_with("entity_008"))
        .unwrap_or_else(|| panic!("{text}"));
    let fields: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(fields, ["entity_008", "2", "2", "style", "yes"], "{text}");
    assert!(text.contains("in 2 file(s)"), "{text}");
}

#[test]
fn range_limits_fixing_to_the_given_lines() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = "entity e is\nend entity e;\n\narchitecture a of e is\n\nbegin\n\n  x<=y;\n  z<=w;\n\nend architecture a;\n";
    let file = write(dir.path(), "a.vhd", src);
    let out = vsg(
        &["--fix", "--range", "8:8", "-f", file.to_str().unwrap()],
        "",
    );
    assert!(out.status.success(), "{out:?}");
    let fixed = std::fs::read_to_string(&file).unwrap();
    assert!(fixed.contains("  x <= y;"), "{fixed}");
    // The line outside the range keeps its spacing.
    assert!(fixed.contains("  z<=w;"), "{fixed}");
}

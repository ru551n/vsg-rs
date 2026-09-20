//! The VSG command line: the arguments, reports and exit codes of VSG 3.35, with one
//! difference: there are no phases. Every violation is reported at once (`-fp` and `-ap` are
//! accepted and have no effect) and `--fix` fixes everything in one run.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{CommandFactory, Parser, ValueEnum};
use rayon::prelude::*;
use vsg_rs::config::Config;
use vsg_rs::rules::{self, Project, Violation};
use vsg_rs::{FixOptions, FormatError, Parsed};

use crate::local_rules::LocalRules;

/// One edit of a safe fix, as positions rather than byte offsets, so a consumer that has only
/// the report can apply it without reading the source again.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Replacement {
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
    text: String,
}

/// One reported violation.
#[derive(serde::Serialize, serde::Deserialize)]
struct Diagnostic {
    line: usize,
    column: usize,
    rule: String,
    /// `error` or `warning`.
    severity: String,
    message: String,
    /// Whether `--fix` removes this one without anybody reading it. Per finding rather than per
    /// rule, because the same rule can offer a safe fix in one place and none in another.
    #[serde(default)]
    fixable: bool,
    /// The other places this finding is about; see `lint::Related`.
    #[serde(default)]
    related: Vec<vsg_rs::analysis::lint::Related>,
    /// The edits of this finding's fix, when it has a safe one.
    #[serde(default)]
    fix: Vec<Replacement>,
}

/// Which layer a finding came from, derived from its rule id so that it cannot disagree with
/// what actually produced it: `lint` for the lint layer, `layout` for what the formatter decides,
/// `style` for the rest of VSG's rules.
fn kind_of(rule: &str) -> &'static str {
    if rule.starts_with("lint_") {
        "lint"
    } else if rule == "format" || rules::owner_of(rule) == rules::Owner::Formatter {
        "layout"
    } else {
        "style"
    }
}

/// The layers a run may report, in the order they are printed.
const KINDS: [&str; 3] = ["style", "layout", "lint"];

/// Whether `--check` asked for a layer. The worker sees the same command line as the parent, so
/// both answer this the same way without passing it through the request.
fn wants(args: &Args, layer: &str) -> bool {
    args.check.split(',').any(|l| l.trim() == layer)
}

fn diagnostic(parsed: &Parsed, v: &Violation) -> Diagnostic {
    let (line, column) = parsed.line_col(v.start);
    // Only a fix vsg-rs would apply itself is offered; an unsafe one is a suggestion for a
    // person, and the same rule governs --fix, so the two cannot disagree.
    let safe = v
        .fix
        .as_ref()
        .filter(|f| f.safety == rules::FixSafety::Safe);
    Diagnostic {
        line,
        column,
        rule: v.rule.to_owned(),
        severity: v.severity.to_string(),
        message: v.message.clone(),
        fixable: safe.is_some(),
        // Style rules point at one place; the lint layer fills this in.
        related: Vec::new(),
        // Positions are resolved here, while the source that produced the offsets is at hand.
        fix: safe
            .map(|f| {
                // `--fix` applies edits in (start, rank, end) order, and two of them can insert
                // at the same offset. A consumer sees positions only, so the order is applied
                // here and same-offset insertions are merged into one replacement -- otherwise
                // `end;` becomes `end e entity;` instead of `end entity e;`.
                let mut edits: Vec<&rules::Edit> = f.edits.iter().collect();
                edits.sort_by_key(|e| (e.start, e.rank, e.end));
                let mut merged: Vec<rules::Edit> = Vec::new();
                for edit in edits {
                    match merged.last_mut() {
                        Some(last) if last.start == edit.start && last.end == edit.end => {
                            last.text.push_str(&edit.text);
                        }
                        _ => merged.push((*edit).clone()),
                    }
                }
                merged
                    .iter()
                    .map(|e| {
                        let (line, column) = parsed.line_col(e.start);
                        let (end_line, end_column) = parsed.line_col(e.end);
                        Replacement {
                            line,
                            column,
                            end_line,
                            end_column,
                            text: e.text.clone(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn describe_error(parsed: &Parsed, e: &FormatError) -> String {
    let at = |offset| {
        let (line, col) = parsed.line_col(offset);
        format!("{line}:{col}")
    };
    match e {
        FormatError::Syntax(diags) => {
            let first = diags
                .first()
                .map(|d| format!(" (first at {}: {})", at(d.offset), d.message))
                .unwrap_or_default();
            format!("{e}{first}; left unchanged")
        }
        FormatError::Unsupported(d) => format!("{}: {e}; left unchanged", at(d.offset)),
        FormatError::Internal(_) => format!("{e}; left unchanged (please report this)"),
    }
}

/// Violations for what formatting changes, under the VSG rule that reports each change.
fn layout_findings(parsed: &Parsed, formatted: Vec<u8>, cfg: &Config) -> Vec<Diagnostic> {
    let after = Parsed::new(formatted);
    let mut out: Vec<Diagnostic> = Vec::new();
    for change in vsg_rs::layout::layout_changes(parsed, &after) {
        let rule = vsg_rs::layout::rule_for(&change);
        // A disabled VSG rule does not report (the formatter still applies its policy).
        if rule != "format" && cfg.rule_by_id(rule).is_some_and(|s| !s.enabled) {
            continue;
        }
        if out.iter().any(|d| d.line == change.line && d.rule == rule) {
            continue;
        }
        out.push(Diagnostic {
            line: change.line,
            column: 1,
            rule: rule.to_owned(),
            severity: "error".into(),
            message: vsg_rs::layout::message(&change, cfg.format.indent),
            fixable: true,
            related: Vec::new(),
            fix: Vec::new(),
        });
    }
    if out.is_empty() {
        // The tokens are the same; only line endings or the final newline differ.
        out.push(Diagnostic {
            line: parsed.source().split(|&b| b == b'\n').count().max(1),
            column: 1,
            rule: "format".into(),
            severity: "error".into(),
            message: "File is not formatted".into(),
            fixable: true,
            related: Vec::new(),
            fix: Vec::new(),
        });
    }
    out
}

/// Replace `path` with `contents` without ever leaving a partially written file.
fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    tmp.as_file()
        .set_permissions(std::fs::metadata(path)?.permissions())?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// 64-bit FNV-1a, stable across platforms and releases.
fn fnv1a(bytes: impl IntoIterator<Item = u8>, seed: u64) -> u64 {
    bytes.into_iter().fold(seed, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// A stable 128-bit hex fingerprint of `key` (GitLab code quality).
fn fingerprint(key: &str) -> String {
    format!(
        "{:016x}{:016x}",
        fnv1a(key.bytes(), 0xcbf2_9ce4_8422_2325),
        fnv1a(key.bytes().rev(), 0x8422_2325_cbf2_9ce4)
    )
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
enum OutputFormat {
    Vsg,
    Syntastic,
    Summary,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Style {
    #[value(name = "indent_only")]
    IndentOnly,
    #[value(name = "jcl")]
    Jcl,
}

#[derive(Parser)]
#[allow(clippy::struct_excessive_bools)] // VSG's flags.
#[command(
    name = "VHDL Style Guide (VSG)",
    bin_name = "vsg-rs",
    about = "Analyzes VHDL files for style guide violations. Reference documentation is \
             located at: http://vhdl-style-guide.readthedocs.io/en/latest/index.html",
    disable_version_flag = true
)]
struct Args {
    /// File to analyze
    #[arg(value_name = "FILENAME")]
    positional: Vec<PathBuf>,
    /// File to analyze
    #[arg(short = 'f', long = "filename", value_name = "FILENAME", num_args = 1..)]
    filename: Vec<PathBuf>,
    /// Path to local rules
    #[arg(long = "local_rules", value_name = "LOCAL_RULES")]
    local_rules: Option<PathBuf>,
    /// JSON or YAML configuration file(s)
    #[arg(short = 'c', long = "configuration", value_name = "CONFIGURATION", num_args = 1..)]
    configuration: Vec<PathBuf>,
    /// Fix issues found
    #[arg(long)]
    fix: bool,
    /// Fix issues up to and including this phase (vsg-rs has one phase)
    #[arg(long = "fix_phase", value_name = "FIX_PHASE")]
    fix_phase: Option<u32>,
    /// Extract Junit file
    #[arg(short = 'j', long = "junit", value_name = "JUNIT")]
    junit: Option<PathBuf>,
    /// Extract JSON file
    #[arg(long = "json", value_name = "JSON")]
    json: Option<PathBuf>,
    /// Sets the output format.
    #[arg(long = "output_format", value_enum, default_value = "vsg")]
    output_format: OutputFormat,
    /// Creates a copy of input file for comparison with fixed version.
    #[arg(short = 'b', long)]
    backup: bool,
    /// Write configuration to file name.
    #[arg(long = "output_configuration", value_name = "OUTPUT_CONFIGURATION")]
    output_configuration: Option<PathBuf>,
    /// Display configuration of a rule
    #[arg(long = "rule_configuration", value_name = "RULE_CONFIGURATION")]
    rule_configuration: Option<String>,
    /// Use predefined style
    #[arg(long, value_enum)]
    style: Option<Style>,
    /// Displays version information
    #[arg(short = 'v', long)]
    version: bool,
    /// Do not stop when a violation is detected (always the case in vsg-rs)
    #[arg(long = "all_phases")]
    all_phases: bool,
    /// Restrict fixing via JSON file.
    #[arg(long = "fix_only", value_name = "FIX_ONLY")]
    fix_only: Option<PathBuf>,
    /// Read VHDL input from stdin, disables all other file selections, disables
    /// multiprocessing
    #[arg(long)]
    stdin: bool,
    /// Apply fixes if syntax errors are detected (no effect: such files are never changed)
    #[arg(long = "force_fix")]
    force_fix: bool,
    /// Create code quality report for GitLab
    #[arg(long = "quality_report", value_name = "QUALITY_REPORT")]
    quality_report: Option<PathBuf>,
    /// Write SonarQube generic issue JSON (sonar.externalIssuesReportPaths)
    #[arg(long = "sonarqube", value_name = "SONARQUBE")]
    sonarqube: Option<PathBuf>,
    /// number of parallel jobs to use, default is the number of cpu cores
    #[arg(short = 'p', long, value_name = "JOBS")]
    jobs: Option<usize>,
    /// Displays verbose debug information
    #[arg(long)]
    debug: bool,
    /// With --fix, also apply fixes that VSG does not apply by default; they may change
    /// behaviour or remove information (vsg-rs extension)
    #[arg(long = "unsafe_fixes")]
    unsafe_fixes: bool,
    /// With --fix, print a unified diff instead of changing files (vsg-rs extension)
    #[arg(long)]
    diff: bool,
    /// With --fix, change only lines START to END, 1-based (vsg-rs extension)
    #[arg(long, value_name = "START:END", value_parser = parse_line_range)]
    range: Option<(usize, usize)>,
    /// Path of the --stdin input, for configuration lookup and reports (vsg-rs extension)
    #[arg(long = "stdin_filename", value_name = "PATH")]
    stdin_filename: Option<PathBuf>,
    /// Extract SARIF 2.1.0 file for code scanning (vsg-rs extension)
    #[arg(long, value_name = "SARIF")]
    sarif: Option<PathBuf>,
    /// Check the VHDL files (.vhd, .vhdl) in directories and their subdirectories (vsg-rs
    /// extension)
    #[arg(long)]
    recursive: bool,
    /// List every VSG rule and how vsg-rs handles it (vsg-rs extension)
    #[arg(long = "list_rules")]
    list_rules: bool,
    /// Print how many violations each rule reports, over all inputs (vsg-rs extension)
    #[arg(long)]
    statistics: bool,
    /// Configuration applied to the lint layer only, after `-c`. Several files are merged in
    /// order (vsg-rs extension)
    #[arg(long = "lint_configuration", value_name = "LINT_CONFIGURATION", num_args = 1..)]
    lint_configuration: Vec<PathBuf>,
    /// Describe one rule: what it checks, which layer it belongs to and whether it is fixed
    /// (vsg-rs extension)
    #[arg(long = "explain", value_name = "RULE")]
    explain: Option<String>,
    /// Which layers make the run fail, comma separated: `style`, `layout`, `lint`. By default
    /// any error-severity violation does (vsg-rs extension)
    #[arg(long = "fail_on", value_name = "LAYERS")]
    fail_on: Option<String>,
    /// Which layers to run, comma separated: `style` (VSG's rules, the default) and `lint`
    /// (rules that need name resolution). `vsg-rs lint ...` is the short way to say
    /// `--check lint` (vsg-rs extension)
    #[arg(long = "check", value_name = "LAYERS", default_value = "style")]
    check: String,
    /// Accept the violations listed in this file, with their reasons (vsg-rs extension)
    #[arg(long = "waivers", value_name = "WAIVERS", num_args = 1..)]
    waivers: Vec<PathBuf>,
    /// Write a waiver file accepting every violation found now (vsg-rs extension)
    #[arg(long = "generate_waivers", value_name = "FILE")]
    generate_waivers: Option<PathBuf>,
    /// List the waived violations instead of only counting them (vsg-rs extension)
    #[arg(long = "show_waived")]
    show_waived: bool,
}

/// Parse `START:END` (1-based, inclusive line numbers).
fn parse_line_range(s: &str) -> Result<(usize, usize), String> {
    let (a, b) = s.split_once(':').ok_or("expected START:END")?;
    let a: usize = a.trim().parse().map_err(|e| format!("START: {e}"))?;
    let b: usize = b.trim().parse().map_err(|e| format!("END: {e}"))?;
    if a == 0 || b < a {
        return Err("expected 1 <= START <= END".into());
    }
    Ok((a, b))
}

/// Byte range of 1-based lines `first..=last`.
fn line_range(src: &[u8], (first, last): (usize, usize)) -> std::ops::Range<usize> {
    let start = |n: usize| match n.checked_sub(2) {
        None => 0,
        Some(k) => src
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b == b'\n')
            .nth(k)
            .map_or(src.len(), |(i, _)| i + 1),
    };
    start(first)..start(last + 1)
}

/// VSG's multi-letter single-dash options, as long options.
/// The root command is VSG's: same arguments, same reports, style rules only. `vsg-rs lint ...`
/// runs the lint layer instead, and is the same thing as `--check lint`.
///
/// VSG has no subcommands and its positional arguments are file names, so the word is only taken
/// as a subcommand when it is the first argument and no file of that name exists; a file called
/// `lint` still wins. An explicit `--check` is left alone, so `vsg-rs lint --check style,lint`
/// runs both layers.
fn lint_subcommand(args: &mut Vec<String>) {
    let Some(first) = args.get(1) else {
        return;
    };
    if first != "lint" || Path::new(first).exists() {
        return;
    }
    args.remove(1);
    if !args
        .iter()
        .any(|a| a == "--check" || a.starts_with("--check="))
    {
        args.insert(1, "--check".to_owned());
        args.insert(2, "lint".to_owned());
    }
}

fn normalize(args: impl Iterator<Item = String>) -> Vec<String> {
    let mut args: Vec<String> = args
        .map(|a| {
            let long = match a.as_str() {
                "-lr" => "--local_rules",
                "-fp" => "--fix_phase",
                "-js" => "--json",
                "-of" => "--output_format",
                "-oc" => "--output_configuration",
                "-rc" => "--rule_configuration",
                "-lc" => "--lint_configuration",
                "-ap" => "--all_phases",
                _ => return a,
            };
            long.to_owned()
        })
        .collect();
    lint_subcommand(&mut args);
    args
}

/// What `lint_602` (suffix) and `lint_603` (prefix) accept. Either rule enabled without a list
/// of its own means the usual convention for it.
fn usage_error(message: &str) -> ExitCode {
    let mut cmd = Args::command();
    eprintln!("{}", cmd.render_usage());
    eprintln!("VHDL Style Guide (VSG): error: {message}");
    ExitCode::from(1)
}

/// Renders a report file from all results.
type Render = fn(&[FileResult]) -> String;

/// The result for one input.
#[derive(serde::Serialize, serde::Deserialize)]
struct FileResult {
    name: String,
    violations: Vec<Diagnostic>,
    error: Option<String>,
    output: Option<Vec<u8>>,
}

fn capitalized(severity: &str) -> &'static str {
    if severity == "warning" {
        "Warning"
    } else {
        "Error"
    }
}

fn check(
    name: &str,
    source: Vec<u8>,
    cfg: &Config,
    args: &Args,
    options: &FixOptions,
) -> FileResult {
    let parsed = Parsed::new(source);
    let mut result = FileResult {
        name: name.to_owned(),
        violations: Vec::new(),
        error: None,
        output: None,
    };
    // A file with only comments has nothing to check but still formats.
    if !parsed.syntax_errors().is_empty() && !parsed.is_blank() {
        let e = vsg_rs::FormatError::Syntax(parsed.syntax_errors().to_vec());
        result.error = Some(describe_error(&parsed, &e));
        return result;
    }
    // Only the lint layer was asked for. The parse above is all this file needs -- it has
    // already reported any syntax error -- and running the style rules, formatting the file and
    // re-parsing the result would produce violations that are thrown away further down.
    if !wants(args, "style") {
        return result;
    }
    let indent_only = args.style == Some(Style::IndentOnly);
    if let (true, Some(lines)) = (args.fix, args.range) {
        let range = line_range(parsed.source(), lines);
        match vsg_rs::fix_range(&parsed, cfg, options, range) {
            Ok(edits) => result.output = Some(vsg_rs::apply_edits(parsed.source(), &edits)),
            Err(e) => result.error = Some(describe_error(&parsed, &e)),
        }
        return result;
    }
    if args.fix {
        let fixed = if indent_only {
            vsg_rs::reindent(&parsed, &cfg.format).map(|out| (out, Vec::new()))
        } else {
            vsg_rs::fix_with(&parsed, cfg, options).map(|o| {
                let fixed = Parsed::new(o.output.clone());
                let remaining = o.remaining.iter().map(|v| diagnostic(&fixed, v)).collect();
                (o.output, remaining)
            })
        };
        match fixed {
            Ok((output, remaining)) => {
                result.violations = remaining;
                result.output = Some(output);
            }
            Err(e) => result.error = Some(describe_error(&parsed, &e)),
        }
        return result;
    }
    let formatted = if indent_only {
        vsg_rs::reindent(&parsed, &cfg.format)
    } else {
        let (violations, formatted) =
            rules::check_and_format_with(&parsed, cfg, options.project.as_deref());
        result.violations = violations.iter().map(|v| diagnostic(&parsed, v)).collect();
        formatted
    };
    match formatted {
        Ok(formatted) if formatted != parsed.source() => {
            result
                .violations
                .extend(layout_findings(&parsed, formatted, cfg));
        }
        Ok(_) => {}
        Err(e) => result.error = Some(describe_error(&parsed, &e)),
    }
    result
}

/// Console order: by line, then rule.
fn by_line(r: &FileResult) -> Vec<&Diagnostic> {
    let mut v: Vec<&Diagnostic> = r.violations.iter().collect();
    v.sort_by(|a, b| (a.line, &a.rule).cmp(&(b.line, &b.rule)));
    v
}

/// File report order: by rule, then line.
fn by_rule(r: &FileResult) -> Vec<&Diagnostic> {
    let mut v: Vec<&Diagnostic> = r.violations.iter().collect();
    v.sort_by(|a, b| (&a.rule, a.line).cmp(&(&b.rule, b.line)));
    v
}

fn counts(r: &FileResult) -> (usize, usize) {
    let errors = r
        .violations
        .iter()
        .filter(|d| d.severity != "warning")
        .count();
    (errors, r.violations.len() - errors)
}

fn vsg_report(out: &mut String, r: &FileResult, rules_checked: usize) {
    use std::fmt::Write as _;
    let banner = "=".repeat(80);
    let (errors, warnings) = counts(r);
    let _ = writeln!(out, "{banner}\nFile:  {}\n{banner}", r.name);
    let _ = writeln!(out, "Phase 1 of 1... Reporting");
    let _ = writeln!(out, "Total Rules Checked: {rules_checked}");
    let _ = writeln!(out, "Total Violations: {:>4}", r.violations.len());
    let _ = writeln!(out, "  Error   : {errors:>5}");
    let _ = writeln!(out, "  Warning : {warnings:>5}");
    if r.violations.is_empty() {
        out.push('\n');
        return;
    }
    let width = r
        .violations
        .iter()
        .map(|d| d.rule.len())
        .max()
        .unwrap_or(0)
        .max(4)
        + 1;
    let separator = format!(
        "{}+------------+------------+{}",
        "-".repeat(width + 2),
        "-".repeat(38)
    );
    let _ = writeln!(out, "{separator}");
    let _ = writeln!(
        out,
        "  {:width$}|  severity  |  line(s)   | Solution",
        "Rule"
    );
    let _ = writeln!(out, "{separator}");
    for d in by_line(r) {
        let _ = writeln!(
            out,
            "  {:width$}| {:11}|{:>11} | {}",
            d.rule,
            capitalized(&d.severity),
            d.line,
            d.message
        );
        // The other places the finding is about, one per line under it. A style rule has none,
        // so a run without the lint layer prints exactly what it printed before.
        for related in &d.related {
            let _ = writeln!(
                out,
                "  {:width$}| {:11}|{:>11} |   {}",
                "", "", related.line, related.message
            );
        }
    }
    let _ = writeln!(out, "{separator}");
    let _ = writeln!(
        out,
        "NOTE: Refer to online documentation at \
         https://vhdl-style-guide.readthedocs.io/en/latest/index.html for more information."
    );
}

fn syntastic_report(out: &mut String, r: &FileResult) {
    use std::fmt::Write as _;
    for d in by_line(r) {
        let _ = writeln!(
            out,
            "{}: {}({}){} -- {}",
            capitalized(&d.severity).to_uppercase(),
            r.name,
            d.line,
            d.rule,
            d.message
        );
    }
}

fn summary_report(out: &mut String, r: &FileResult, rules_checked: usize) {
    use std::fmt::Write as _;
    let (errors, warnings) = counts(r);
    let status = if errors > 0 { "ERROR" } else { "OK" };
    let _ = writeln!(
        out,
        "File: {} {status} ({rules_checked} rules checked) [Error: {errors}] [Warning: {warnings}]",
        r.name
    );
}

/// `--statistics`: violations per rule over all inputs, most first.
fn statistics_report(results: &[FileResult], cfg: &Config) -> String {
    use std::fmt::Write as _;
    let mut counts: std::collections::HashMap<&str, (usize, std::collections::HashSet<&str>)> =
        std::collections::HashMap::new();
    for r in results {
        for d in &r.violations {
            let entry = counts.entry(&d.rule).or_default();
            entry.0 += 1;
            entry.1.insert(&r.name);
        }
    }
    let mut rows: Vec<(&str, usize, usize)> = counts
        .iter()
        .map(|(rule, (n, files))| (*rule, *n, files.len()))
        .collect();
    rows.sort_by(|a, b| (b.1, a.0).cmp(&(a.1, b.0)));
    let width = rows.iter().map(|(r, ..)| r.len()).max().unwrap_or(4).max(4);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:width$}  {:>10}  {:>5}  {:<6}  --fix",
        "rule", "violations", "files", "layer"
    );
    for (rule, n, files) in &rows {
        // Layout is always fixed by formatting, and a rule is fixed if its fixes are enabled;
        // a lint finding never carries a fix, because fixing one would change the design.
        let fixed = if kind_of(rule) == "lint" {
            "no"
        } else if is_layout(rule) || cfg.rule_by_id(rule).is_some_and(|s| s.fixable) {
            "yes"
        } else {
            "no"
        };
        let _ = writeln!(
            out,
            "{rule:width$}  {n:>10}  {files:>5}  {:<6}  {fixed}",
            kind_of(rule)
        );
    }
    let total: usize = rows.iter().map(|(_, n, _)| n).sum();
    let files = results.iter().filter(|r| !r.violations.is_empty()).count();
    let per_layer: Vec<String> = KINDS
        .iter()
        .filter_map(|kind| {
            let n: usize = rows
                .iter()
                .filter(|(rule, ..)| kind_of(rule) == *kind)
                .map(|(_, n, _)| n)
                .sum();
            (n > 0).then(|| format!("{n} {kind}"))
        })
        .collect();
    let _ = writeln!(
        out,
        "\n{total} violation(s) of {} rule(s) in {files} file(s){}",
        rows.len(),
        if per_layer.is_empty() {
            String::new()
        } else {
            format!(" ({})", per_layer.join(", "))
        }
    );
    out
}

fn json_report(results: &[FileResult]) -> String {
    let files: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            let violations: Vec<serde_json::Value> = by_rule(r)
                .into_iter()
                .map(|d| {
                    serde_json::json!({
                        "rule": d.rule,
                        "linenumber": d.line,
                        "severity": capitalized(&d.severity),
                        "solution": d.message,
                    })
                })
                .collect();
            serde_json::json!({ "file_path": r.name, "violations": violations })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({ "files": files })).unwrap_or_default()
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The current UTC time as `YYYY-MM-DDTHH:MM:SS`.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rest = secs % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "localhost".into())
}

fn junit_report(results: &[FileResult]) -> String {
    use std::fmt::Write as _;
    let failures = results.iter().filter(|r| !r.violations.is_empty()).count();
    let mut out = String::from("<?xml version=\"1.0\" ?>\n");
    let _ = writeln!(
        out,
        "<testsuite errors=\"0\" hostname=\"{}\" failures=\"{failures}\" timestamp=\"{}\" \
         tests=\"{}\" time=\"0\" name=\"vhdl-style-guide\">",
        xml_escape(&hostname()),
        timestamp(),
        results.len()
    );
    out.push_str("  <properties>\n  </properties>\n");
    for r in results {
        let _ = writeln!(
            out,
            "  <testcase name=\"{}\" time=\"0\">",
            xml_escape(&r.name)
        );
        if !r.violations.is_empty() {
            out.push_str("    <failure type=\"Failure\">\n");
            for d in by_rule(r) {
                let _ = writeln!(
                    out,
                    "      {}: {} : {}",
                    d.rule,
                    d.line,
                    xml_escape(&d.message)
                );
            }
            out.push_str("    </failure>\n");
        }
        out.push_str("  </testcase>\n");
    }
    out.push_str(
        "  <system-out>\n  </system-out>\n  <system-err>\n  </system-err>\n</testsuite>\n",
    );
    out
}

fn quality_report(results: &[FileResult]) -> String {
    let issues: Vec<serde_json::Value> = results
        .iter()
        .flat_map(|r| by_rule(r).into_iter().map(move |d| (r, d)))
        .map(|(r, d)| {
            let key = format!("{}:{}:{}:{}", r.name, d.rule, d.line, d.message);
            serde_json::json!({
                "description": format!("{} :: {}", d.rule, d.message),
                "fingerprint": fingerprint(&key),
                "severity": if d.severity == "warning" { "minor" } else { "critical" },
                "location": { "path": r.name, "lines": { "begin": d.line } },
            })
        })
        .collect();
    serde_json::to_string_pretty(&issues).unwrap_or_default()
}

/// How much a finding should weigh in SonarQube, which grades on five levels where vsg-rs has
/// two. The layer supplies what the severity alone cannot:
///
/// * anything `--fix` repairs on its own is `INFO`, because one run removes all of it at once.
///   Without this, a large code base arrives as tens of thousands of items indistinguishable
///   from the ones a person has to sit down and think about, and the few that need a decision
///   are buried by the count. This is asked of each finding rather than of its rule, because the
///   same rule can offer a safe fix in one place and none in another.
/// * a lint finding is `CRITICAL`: a latch, two drivers or a clock crossing is a defect in the
///   hardware, not an opinion about it.
/// * a rule configured as a warning stays `MINOR`, since that is the project saying so.
///
/// The exit code is unaffected: a fixable violation still fails the run, it just does not
/// pretend to be technical debt.
fn sonar_severity(kind: &str, severity: &str, fixable: bool) -> &'static str {
    match (kind, severity, fixable) {
        (_, _, true) => "INFO",
        (_, "warning", _) => "MINOR",
        ("lint", _, _) => "CRITICAL",
        _ => "MAJOR",
    }
}

/// SonarQube's generic issue format, for `sonar.externalIssuesReportPaths`.
///
/// SonarQube also reads SARIF, which vsg-rs already writes, but it files every SARIF issue as a
/// vulnerability. A style violation is not a security finding, and a few hundred of them would
/// bury the project's real ones. This format carries the type, so the lint layer arrives as a
/// bug and everything else as a code smell.
fn sonar_report(results: &[FileResult]) -> String {
    let issues: Vec<serde_json::Value> = results
        .iter()
        .flat_map(|r| by_rule(r).into_iter().map(move |d| (r, d)))
        .map(|(r, d)| {
            let mut range = serde_json::json!({ "startLine": d.line });
            // SonarQube counts columns from zero, and every report format here from one.
            if let Some(column) = d.column.checked_sub(1)
                && let Some(range) = range.as_object_mut()
            {
                range.insert("startColumn".to_owned(), column.into());
            }
            let kind = kind_of(&d.rule);
            serde_json::json!({
                "engineId": "vsg-rs",
                "ruleId": d.rule,
                // A lint finding says the hardware is wrong; a style one says it reads badly.
                "type": if kind == "lint" { "BUG" } else { "CODE_SMELL" },
                "severity": sonar_severity(kind, &d.severity, d.fixable),
                "primaryLocation": {
                    "message": d.message,
                    "filePath": r.name,
                    "textRange": range,
                },
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({ "issues": issues })).unwrap_or_default()
}

/// A file name as a SARIF URI: relative to the working directory with `/` separators (as code
/// scanning expects), or a `file://` URI for files outside it.
fn sarif_uri(name: &str) -> String {
    let path = Path::new(name);
    let relative = std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf))
        .unwrap_or_else(|| path.to_path_buf());
    let text = relative.to_string_lossy().replace('\\', "/");
    let text = text.trim_start_matches("./");
    if relative.is_absolute() {
        format!(
            "file://{}{text}",
            if text.starts_with('/') { "" } else { "/" }
        )
    } else {
        text.to_owned()
    }
}

/// SARIF metadata of a rule: code scanning shows the name and description with each alert.
fn sarif_rule(id: &str) -> serde_json::Value {
    let description = match (rules::info(id), id) {
        (Some(info), _) => info.description.to_owned(),
        (None, "format") => "Layout that differs from the formatted file (run --fix)".to_owned(),
        (None, "source_file_001") => "Input files exist and can be read".to_owned(),
        (None, _) => "Layout checked by the formatter (run --fix)".to_owned(),
    };
    let mut rule = serde_json::json!({
        "id": id,
        "name": id,
        "shortDescription": { "text": description },
    });
    if let Some((prefix, number)) = id.rsplit_once('_')
        && vsg_rs::vsg_defaults::rule_ids().any(|known| known == id)
    {
        rule["helpUri"] = format!(
            "https://vhdl-style-guide.readthedocs.io/en/latest/{prefix}_rules.html#{}-{number}",
            prefix.replace('_', "-")
        )
        .into();
    }
    rule
}

/// Whether a finding is about layout (reported by the formatter), not a rule violation.
fn is_layout(rule: &str) -> bool {
    rule == "format"
        || (rules::info(rule).is_none()
            && rules::vsg_catalog()
                .any(|(id, owner)| id == rule && owner == rules::Owner::Formatter))
}

/// One SARIF result.
/// What a diagnostic adds to its SARIF result beyond the primary location. A reformat block
/// stands for several findings and has neither.
#[derive(Default)]
struct SarifExtras<'a> {
    related: &'a [vsg_rs::analysis::lint::Related],
    fix: &'a [Replacement],
}

fn sarif_result(
    r: &FileResult,
    rule: &str,
    level: &str,
    message: &str,
    lines: (usize, usize),
    column: usize,
    extras: &SarifExtras<'_>,
) -> serde_json::Value {
    let (related, fix) = (extras.related, extras.fix);
    let mut result = serde_json::json!({
        "ruleId": rule,
        "level": level,
        "message": { "text": message },
        "locations": [{
            "physicalLocation": {
                "artifactLocation": { "uri": sarif_uri(&r.name) },
                "region": {
                    "startLine": lines.0.max(1),
                    "endLine": lines.1.max(1),
                    "startColumn": column.max(1)
                }
            }
        }]
    });
    let Some(object) = result.as_object_mut() else {
        return result;
    };
    // The other places the finding is about. Code scanning links each one.
    if !related.is_empty() {
        let locations: Vec<serde_json::Value> = related
            .iter()
            .map(|other| {
                serde_json::json!({
                    "physicalLocation": {
                        "artifactLocation": {
                            "uri": sarif_uri(&other.file.display().to_string())
                        },
                        "region": {
                            "startLine": other.line.max(1),
                            "startColumn": other.column.max(1)
                        }
                    },
                    "message": { "text": other.message }
                })
            })
            .collect();
        object.insert("relatedLocations".to_owned(), locations.into());
    }
    // Only fixes vsg-rs would apply itself, so what a consumer offers matches `--fix`.
    if !fix.is_empty() {
        let replacements: Vec<serde_json::Value> = fix
            .iter()
            .map(|edit| {
                serde_json::json!({
                    "deletedRegion": {
                        "startLine": edit.line.max(1),
                        "startColumn": edit.column.max(1),
                        "endLine": edit.end_line.max(1),
                        "endColumn": edit.end_column.max(1)
                    },
                    "insertedContent": { "text": edit.text }
                })
            })
            .collect();
        object.insert(
            "fixes".to_owned(),
            serde_json::json!([{
                "description": { "text": format!("Apply the {rule} fix (vsg-rs --fix)") },
                "artifactChanges": [{
                    "artifactLocation": { "uri": sarif_uri(&r.name) },
                    "replacements": replacements
                }]
            }]),
        );
    }
    result
}

/// SARIF 2.1.0: one result per rule violation, and one `format` result per block of adjacent
/// lines with layout findings, so that code scanning shows one alert per block to reformat.
fn sarif_report(results: &[FileResult]) -> String {
    let mut rules: Vec<&str> = Vec::new();
    let mut findings: Vec<serde_json::Value> = Vec::new();
    for r in results {
        let mut block: Option<(usize, usize, Vec<&str>, bool)> = None;
        let flush = |block: &mut Option<(usize, usize, Vec<&str>, bool)>,
                     findings: &mut Vec<serde_json::Value>| {
            if let Some((first, last, mut kinds, error)) = block.take() {
                kinds.retain(|k| *k != "format");
                kinds.sort_unstable();
                kinds.dedup();
                let lines = if first == last {
                    format!("line {first}")
                } else {
                    format!("lines {first}-{last}")
                };
                let message = if kinds.is_empty() {
                    format!("Reformat {lines} (vsg-rs --fix)")
                } else {
                    format!(
                        "Reformat {lines} (vsg-rs --fix); VSG rules: {}",
                        kinds.join(", ")
                    )
                };
                let level = if error { "error" } else { "warning" };
                // A reformat block stands for several findings; its fix is `--fix` on the file.
                findings.push(sarif_result(
                    r,
                    "format",
                    level,
                    &message,
                    (first, last),
                    1,
                    &SarifExtras::default(),
                ));
            }
        };
        for d in by_line(r) {
            let level = if d.severity == "warning" {
                "warning"
            } else {
                "error"
            };
            if !is_layout(&d.rule) {
                rules.push(&d.rule);
                findings.push(sarif_result(
                    r,
                    &d.rule,
                    level,
                    &d.message,
                    (d.line, d.line),
                    d.column,
                    &SarifExtras {
                        related: &d.related,
                        fix: &d.fix,
                    },
                ));
                continue;
            }
            rules.push("format");
            match &mut block {
                Some((_, last, kinds, error)) if d.line <= *last + 1 => {
                    *last = (*last).max(d.line);
                    kinds.push(&d.rule);
                    *error |= level == "error";
                }
                _ => {
                    flush(&mut block, &mut findings);
                    block = Some((d.line, d.line, vec![d.rule.as_str()], level == "error"));
                }
            }
        }
        flush(&mut block, &mut findings);
    }
    rules.sort_unstable();
    rules.dedup();
    let doc = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "vsg-rs",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/ru551n/vsg-rs",
                    "rules": rules.iter().map(|r| sarif_rule(r)).collect::<Vec<_>>(),
                }
            },
            "results": findings,
        }]
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

/// `--explain RULE`: what one rule is, in the words vsg-rs has for it.
/// Every rule of the lint layer: the front end's own, and the structural ones vsg-rs adds.
/// One list, so `--list_rules` and `--explain` can never disagree about what exists.
fn lint_rules() -> impl Iterator<Item = (&'static str, &'static str)> {
    vsg_rs::analysis::lint::rules()
        .chain(vsg_rs::analysis::design::RULES.iter().copied())
        .chain(vsg_rs::analysis::elaborate::RULES.iter().copied())
        .chain(vsg_rs::analysis::fsm::RULES.iter().copied())
        .chain(vsg_rs::analysis::combinational::RULES.iter().copied())
        .chain(vsg_rs::analysis::clockdomain::RULES.iter().copied())
        .chain(vsg_rs::analysis::width::RULES.iter().copied())
        .chain(vsg_rs::analysis::choices::RULES.iter().copied())
        .chain(vsg_rs::analysis::calls::RULES.iter().copied())
}

fn explain_rule(rule: &str) -> ExitCode {
    let mut out = io::stdout().lock();
    let kind = kind_of(rule);
    let described = lint_rules()
        .find(|(id, _)| *id == rule)
        .map(|(_, description)| description.to_owned())
        .or_else(|| rules::info(rule).map(|info| info.description.to_owned()))
        .or_else(|| {
            (kind == "layout" && rules::is_known_rule(rule))
                .then(|| "Layout, decided by the formatter and applied by --fix.".to_owned())
        });
    let Some(description) = described else {
        eprintln!("ERROR: unknown rule `{rule}`; `--list_rules` lists them all");
        return ExitCode::from(1);
    };
    let _ = writeln!(out, "{rule}\n\n{description}\n");
    let _ = writeln!(out, "Layer:     {kind}");
    let _ = writeln!(
        out,
        "Run by:    {}",
        if kind == "lint" {
            "vsg-rs lint (or --check style,lint)"
        } else {
            "vsg-rs (the default layer)"
        }
    );
    let _ = writeln!(
        out,
        "Fixed:     {}",
        match kind {
            "lint" => "no, a lint finding never carries a fix",
            "layout" => "yes, by --fix",
            _ => "only if the rule's fixes are enabled (see --fix)",
        }
    );
    if kind != "lint" {
        let _ = writeln!(
            out,
            "\nVSG documents this rule:\n  https://vhdl-style-guide.readthedocs.io/en/latest/{}_rules.html",
            rule.rsplit_once('_').map_or(rule, |(head, _)| head)
        );
    }
    ExitCode::SUCCESS
}

fn list_rules() -> ExitCode {
    let mut out = io::stdout().lock();
    for (id, owner) in rules::vsg_catalog() {
        let status = match (rules::info(id), owner) {
            (Some(info), _) => format!("rule: {}", info.description),
            (None, rules::Owner::Formatter) => "formatter (applied by --fix)".to_owned(),
            (None, rules::Owner::Cli) => "command line (missing files are reported)".to_owned(),
            (None, other) => format!("not implemented ({other})"),
        };
        let _ = writeln!(out, "{id:42} {status}");
    }
    for (id, description) in lint_rules() {
        let _ = writeln!(out, "{id:42} lint (--check lint): {description}");
    }
    ExitCode::SUCCESS
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn directory_result(name: String) -> FileResult {
    FileResult {
        name,
        violations: vec![Diagnostic {
            line: 0,
            column: 0,
            rule: "source_file_001".into(),
            severity: "error".into(),
            message: "Is a directory".into(),
            fixable: false,
            related: Vec::new(),
            fix: Vec::new(),
        }],
        error: None,
        output: None,
    }
}

/// Write a fixed file (with an optional `.bak` copy of the original).
fn write_fixed(path: &Path, output: &[u8], backup: bool) -> Result<(), String> {
    let original = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if output == original.as_slice() {
        return Ok(());
    }
    if backup {
        let mut name = path.as_os_str().to_owned();
        name.push(".bak");
        std::fs::write(&name, &original)
            .map_err(|e| format!("{}: {e}", Path::new(&name).display()))?;
    }
    write_atomically(path, output).map_err(|e| format!("{}: {e}", path.display()))
}

fn fix_options(args: &Args) -> Result<FixOptions, String> {
    let only = match &args.fix_only {
        None => None,
        Some(path) => Some(
            std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|t| FixOptions::parse_fix_only(&t))
                .map_err(|e| format!("{}: {e}", path.display()))?,
        ),
    };
    Ok(FixOptions {
        unsafe_fixes: args.unsafe_fixes,
        only,
        project: None,
    })
}

/// What one worker derives from parsing its chunk once: the style layer's cross-file index and
/// the lint layer's port table. Each is built only if its layer was asked for, so a lint-only run
/// no longer parses every file to build an index it would discard.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Index {
    project: Option<Project>,
    entities: Option<vsg_rs::analysis::elaborate::Entities>,
}

fn index_of(files: &[PathBuf], style: bool, lint: bool) -> Index {
    Index {
        project: style.then(|| project(files)),
        entities: lint.then(|| vsg_rs::analysis::elaborate::entities(files)),
    }
}

/// Merge two port tables the way `elaborate::entities` does within one: the entity's own ports
/// win over a component declaration repeating them, and otherwise the first definition wins.
fn merge_entities(
    mut into: vsg_rs::analysis::elaborate::Entities,
    from: vsg_rs::analysis::elaborate::Entities,
) -> vsg_rs::analysis::elaborate::Entities {
    for (name, ports) in from {
        match into.entry(name) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(ports);
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                if ports.from_entity && !slot.get().from_entity {
                    slot.insert(ports);
                }
            }
        }
    }
    into
}

fn project(files: &[PathBuf]) -> Project {
    files
        .par_iter()
        .filter(|p| p.is_file())
        .filter_map(|p| std::fs::read(p).ok())
        .map(|source| {
            let mut project = Project::new();
            project.add(&Parsed::new(source));
            project
        })
        .reduce(Project::new, Project::merge)
}

/// Add what the local rules report on the final text of each input to its results.
fn check_local_rules(
    local: &LocalRules,
    files: &[PathBuf],
    stdin_source: &[u8],
    args: &Args,
    results: &mut [FileResult],
) -> Result<(), String> {
    let mut targets: Vec<(usize, PathBuf)> = Vec::new();
    for (i, r) in results.iter().enumerate() {
        if r.error.is_some() || r.violations.iter().any(|v| v.rule == "source_file_001") {
            continue;
        }
        let name = if args.stdin {
            args.stdin_filename
                .clone()
                .unwrap_or_else(|| "stdin.vhd".into())
        } else {
            files[i].clone()
        };
        let target = match (&r.output, args.stdin) {
            (Some(text), _) => local.scratch_copy(&format!("out{i}"), &name, text),
            (None, true) => local.scratch_copy("out-stdin", &name, stdin_source),
            (None, false) => Ok(name),
        }
        .map_err(|e| e.to_string())?;
        targets.push((i, target));
    }
    let paths: Vec<PathBuf> = targets.iter().map(|(_, p)| p.clone()).collect();
    let by_path: std::collections::HashMap<String, usize> = targets
        .iter()
        .map(|(i, p)| (p.to_string_lossy().into_owned(), *i))
        .collect();
    for finding in local.run(&paths, false)? {
        if let Some(&i) = by_path.get(&finding.path) {
            results[i].violations.push(Diagnostic {
                line: finding.line,
                column: 1,
                rule: finding.rule,
                severity: finding.severity,
                message: finding.message,
                // A VSG rule plugin reports; vsg-rs does not know how to fix what it found.
                fixable: false,
                related: Vec::new(),
                fix: Vec::new(),
            });
        }
    }
    for r in results.iter_mut() {
        r.violations.sort_by_key(|v| v.line);
    }
    Ok(())
}

/// Set in worker processes (see [`in_workers`]).
const WORKER_ENV: &str = "VSG_RS_WORKER";

/// Check the files at `indices`, in parallel.
fn process(
    files: &[PathBuf],
    sources: &[Option<PathBuf>],
    indices: &[usize],
    cfg: &Config,
    args: &Args,
    options: &FixOptions,
) -> Vec<FileResult> {
    indices
        .par_iter()
        .map(|&i| {
            let path = &files[i];
            let name = path.display().to_string();
            if path.is_dir() {
                return directory_result(name);
            }
            let read_from = sources.get(i).and_then(Option::as_ref).unwrap_or(path);
            match std::fs::read(read_from) {
                Ok(source) => check(&name, source, &cfg.for_path(path), args, options),
                Err(e) => FileResult {
                    name,
                    violations: Vec::new(),
                    error: Some(e.to_string()),
                    output: None,
                },
            }
        })
        .collect()
}

/// The files for each worker process, balanced by size, or nothing when one process is enough.
///
/// The parser interns token texts in one global lock, so threads in one process contend on
/// every token; separate processes do not.
fn worker_chunks(files: &[PathBuf], jobs: Option<usize>) -> Vec<Vec<usize>> {
    let sizes: Vec<u64> = files
        .iter()
        .map(|f| std::fs::metadata(f).map_or(0, |m| m.len()))
        .collect();
    let total: u64 = sizes.iter().sum();
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    // ponytail: a fixed 64 KiB per process keeps startup cost below the work; tune if small
    // runs get slower.
    let by_size = usize::try_from(total / (64 << 10)).unwrap_or(usize::MAX);
    let n = jobs.unwrap_or(cpus).min(files.len()).min(by_size);
    if n < 2 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
    let mut load = vec![0u64; n];
    let mut chunks = vec![Vec::new(); n];
    for i in order {
        let k = (0..n).min_by_key(|&k| load[k]).unwrap_or(0);
        load[k] += sizes[i].max(1);
        chunks[k].push(i);
    }
    chunks
}

/// Run one worker process per chunk with the same command line, sending `request(chunk)` and
/// reading one JSON reply from each. `None` (the caller works in-process) if there are no
/// chunks or a worker fails.
fn in_workers<T: serde::de::DeserializeOwned>(
    command_line: &[String],
    chunks: &[Vec<usize>],
    request: impl Fn(&[usize]) -> serde_json::Value,
) -> Option<Vec<T>> {
    if chunks.is_empty() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let mut children = Vec::new();
    for chunk in chunks {
        let child = std::process::Command::new(&exe)
            .args(command_line.iter().skip(1))
            .env(WORKER_ENV, "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn();
        match child {
            Ok(mut child) => {
                let body = request(chunk).to_string();
                let sent = child
                    .stdin
                    .take()
                    .is_some_and(|mut stdin| stdin.write_all(body.as_bytes()).is_ok());
                children.push((child, sent));
            }
            Err(_) => break,
        }
    }
    let complete = children.len() == chunks.len();
    let mut replies = Vec::new();
    for (mut child, sent) in children {
        if !(complete && sent) {
            let _ = child.kill();
            let _ = child.wait();
            continue;
        }
        let reply = child
            .wait_with_output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice(&o.stdout).ok());
        replies.push(reply);
    }
    replies
        .into_iter()
        .collect::<Option<Vec<T>>>()
        .filter(|r| r.len() == chunks.len())
}

/// What one file's lint pass produces: whether it is a testbench, and its findings. It crosses
/// the worker boundary, so the testbench reason is an owned string rather than a `&'static str`.
#[derive(serde::Serialize, serde::Deserialize)]
struct PerFile {
    kind: Option<(PathBuf, String)>,
    found: Vec<(PathBuf, Diagnostic)>,
}

/// The per-file half of the lint layer: everything that needs only this file plus the design-wide
/// port table. Runs in a worker process, so the parser's global token interner is not shared.
fn lint_files(
    sources: &[vsg_rs::analysis::lint::Source],
    indices: &[usize],
    entities: &vsg_rs::analysis::elaborate::Entities,
    lint_cfg: &Config,
    kind_sources: &vsg_rs::analysis::testbench::Kinds,
) -> Result<Vec<PerFile>, String> {
    // Testbench code gets the `testbench` rule block, hardware the `rtl` one. There are only ever
    // those two, so they are resolved once rather than per file -- including the naming patterns
    // (`lint_602`/`lint_603`), whose regular expressions are compiled here and whose errors are
    // reported before any file is read.
    let settings_for = |kind| {
        let cfg = lint_cfg.for_kind(kind);
        vsg_rs::analysis::naming_rules(&cfg).map(|naming| (cfg, naming))
    };
    let (rtl, testbench) = (settings_for("rtl")?, settings_for("testbench")?);
    Ok(indices
        .iter()
        .map(|&i| {
            let file = &sources[i].path;
            // The buffer if there is one, otherwise what is on disk now, so positions match the
            // file after `--fix` wrote it.
            let source = match &sources[i].text {
                Some(text) => text.clone(),
                None => match std::fs::read(file) {
                    Ok(source) => source,
                    Err(_) => {
                        return PerFile {
                            kind: None,
                            found: Vec::new(),
                        };
                    }
                },
            };
            let parsed = vsg_rs::Parsed::new(source);
            if !parsed.syntax_errors().is_empty() {
                return PerFile {
                    kind: None,
                    found: Vec::new(),
                };
            }
            let reason = vsg_rs::analysis::testbench::classify(&parsed, file, kind_sources);
            let (cfg, naming) = if reason.is_some() { &testbench } else { &rtl };
            let wiring = vsg_rs::analysis::elaborate::undriven(&parsed, file, entities)
                .into_iter()
                .chain(vsg_rs::analysis::elaborate::interfaces(
                    &parsed, file, entities,
                ));
            let found = vsg_rs::analysis::per_file(&parsed, file, naming, &cfg.synchronizers)
                .into_iter()
                .chain(wiring)
                .filter_map(|f| {
                    let settings = cfg.rule_by_id(f.rule);
                    if settings.as_ref().is_some_and(|s| !s.enabled) {
                        return None;
                    }
                    Some((
                        f.file,
                        Diagnostic {
                            line: f.line,
                            column: f.column,
                            rule: f.rule.to_owned(),
                            severity: settings
                                .map_or_else(|| "error".to_owned(), |s| s.severity.to_string()),
                            message: f.message,
                            // A lint finding never carries a fix.
                            fixable: false,
                            related: f.related,
                            fix: Vec::new(),
                        },
                    ))
                })
                .collect();
            PerFile {
                kind: reason.map(|reason| (file.clone(), reason.to_owned())),
                found,
            }
        })
        .collect())
}

/// Worker process: read a request from stdin, answer on stdout.
fn run_worker(files: &[PathBuf], cfg: &Config, args: &Args, mut options: FixOptions) -> ExitCode {
    #[derive(serde::Deserialize)]
    struct Request {
        index: Option<Vec<usize>>,
        check: Option<Vec<usize>>,
        /// The lint layer's per-file pass, with the design-wide facts it needs.
        lint: Option<LintRequest>,
        project: Option<Project>,
        #[serde(default)]
        sources: Vec<Option<PathBuf>>,
    }
    #[derive(serde::Deserialize)]
    struct LintRequest {
        indices: Vec<usize>,
        entities: vsg_rs::analysis::elaborate::Entities,
        /// The configuration files the lint layer reads, already in merge order, so a worker
        /// resolves exactly the configuration the parent would have.
        configuration: Vec<PathBuf>,
        libraries: std::collections::BTreeMap<PathBuf, Vec<String>>,
    }
    let mut input = Vec::new();
    let Ok(request) = io::stdin()
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())
        .and_then(|_| serde_json::from_slice::<Request>(&input).map_err(|e| e.to_string()))
    else {
        return ExitCode::from(2);
    };
    let reply = if let Some(lint) = request.lint {
        let lint_cfg = if lint.configuration.is_empty() {
            std::borrow::Cow::Borrowed(cfg)
        } else {
            match Config::load(&lint.configuration) {
                Ok(merged) => std::borrow::Cow::Owned(merged),
                Err(e) => {
                    eprintln!("ERROR: {e}");
                    return ExitCode::from(2);
                }
            }
        };
        let kind_sources = vsg_rs::analysis::testbench::Kinds {
            patterns: &cfg.testbench_files,
            libraries: &cfg.testbench_libraries,
            of_file: &lint.libraries,
        };
        let sources: Vec<vsg_rs::analysis::lint::Source> = files
            .iter()
            .map(vsg_rs::analysis::lint::Source::file)
            .collect();
        match lint_files(
            &sources,
            &lint.indices,
            &lint.entities,
            &lint_cfg,
            &kind_sources,
        ) {
            Ok(per_file) => serde_json::to_vec(&per_file),
            Err(e) => {
                eprintln!("ERROR: {e}");
                return ExitCode::from(2);
            }
        }
    } else if let Some(indices) = request.index {
        let chosen: Vec<PathBuf> = indices.iter().map(|&i| files[i].clone()).collect();
        serde_json::to_vec(&index_of(
            &chosen,
            wants(args, "style"),
            wants(args, "lint"),
        ))
    } else {
        options.project = request.project.map(Arc::new);
        let indices = request.check.unwrap_or_default();
        serde_json::to_vec(&process(
            files,
            &request.sources,
            &indices,
            cfg,
            args,
            &options,
        ))
    };
    match reply.map(|r| io::stdout().write_all(&r)) {
        Ok(Ok(())) => ExitCode::SUCCESS,
        _ => ExitCode::from(2),
    }
}

pub(crate) fn main(command_line: &[String]) -> ExitCode {
    // `vsg-rs lsp` serves a language server instead of checking files. As with `lint`, a file of
    // that name still wins, so no VSG command line changes meaning.
    if command_line.get(1).is_some_and(|a| a == "lsp") && !Path::new("lsp").exists() {
        return crate::lsp::serve();
    }
    let worker = std::env::var_os(WORKER_ENV).is_some();
    let args = match Args::try_parse_from(normalize(command_line.iter().cloned())) {
        Ok(args) => args,
        Err(e) => e.exit(),
    };
    if args.version {
        println!(
            "vsg-rs version: {} (VHDL Style Guide (VSG) 3.35 compatible)",
            env!("CARGO_PKG_VERSION")
        );
        return ExitCode::SUCCESS;
    }
    let layers: Vec<String> = args.check.split(',').map(|l| l.trim().to_owned()).collect();
    if let Some(unknown) = layers
        .iter()
        .find(|l| !["style", "lint"].contains(&l.as_str()))
    {
        eprintln!("ERROR: --check: unknown layer `{unknown}` (style, lint)");
        return ExitCode::from(1);
    }
    let style = wants(&args, "style");
    let lint = wants(&args, "lint");
    let gate: Option<Vec<String>> = match &args.fail_on {
        Some(text) => {
            let layers: Vec<String> = text.split(',').map(|l| l.trim().to_owned()).collect();
            if let Some(unknown) = layers.iter().find(|l| !KINDS.contains(&l.as_str())) {
                eprintln!(
                    "ERROR: --fail_on: unknown layer `{unknown}` ({})",
                    KINDS.join(", ")
                );
                return ExitCode::from(1);
            }
            Some(layers)
        }
        None => None,
    };
    if !style && (args.fix || args.fix_only.is_some()) {
        eprintln!("ERROR: --fix belongs to the style layer; add `style` to --check");
        return ExitCode::from(1);
    }
    if let Some(rule) = &args.explain {
        return explain_rule(rule);
    }
    if args.list_rules {
        return list_rules();
    }
    if let Some(jobs) = args.jobs.or(worker.then_some(1)) {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(if worker { 1 } else { jobs.max(1) })
            .build_global();
    }
    let mut files: Vec<PathBuf> = args.filename.clone();
    files.extend(args.positional.iter().cloned());
    // Without -c, a vsg-rs.yaml (or .json) next to the input or in a parent directory is used.
    let configuration = if args.configuration.is_empty() {
        let near = if args.stdin {
            args.stdin_filename.clone()
        } else {
            files.first().cloned()
        };
        let dir = near
            .and_then(|p| std::path::absolute(p).ok())
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        vsg_rs::config::discover(&dir).into_iter().collect()
    } else {
        args.configuration.clone()
    };
    let mut cfg = match Config::load(&configuration) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return ExitCode::from(1);
        }
    };
    // As in VSG, `local_rules` in the configuration wins over `-lr`.
    let local_rules_dir = cfg.local_rules.clone().or_else(|| {
        args.local_rules
            .as_deref()
            .map(|d| vsg_rs::config::expand_path(&d.to_string_lossy()))
    });
    let local_rule_setting = |w: &&String| {
        local_rules_dir.is_some()
            && w.ends_with("is not implemented by vsg-rs; its settings are ignored")
    };
    for w in cfg
        .warnings
        .iter()
        .filter(|_| !worker)
        .filter(|w| !local_rule_setting(w))
    {
        eprintln!("WARNING: {w}");
    }
    if args.style == Some(Style::IndentOnly) {
        cfg.enable_only_group("indent");
    }
    if let Some(id) = &args.rule_configuration {
        let Some(value) = cfg.rule_configuration(id) else {
            println!("ERROR: rule {id} was not found.");
            return ExitCode::from(1);
        };
        let doc = serde_json::json!({ "rule": { id.as_str(): value } });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    if let Some(path) = args.output_configuration.as_ref().filter(|_| !worker) {
        let text = serde_json::to_string_pretty(&cfg.effective_configuration()).unwrap_or_default();
        if let Err(e) = write_file(path, &(text + "\n")) {
            eprintln!("ERROR: {e}");
            return ExitCode::from(1);
        }
    }
    let mut options = match fix_options(&args) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return ExitCode::from(1);
        }
    };
    if !args.stdin {
        for (pattern, source) in &cfg.file_list {
            let found = vsg_rs::config::expand_pattern(pattern);
            if found.is_empty() {
                println!(
                    "ERROR: Could not find file {pattern} in configuration file {}",
                    source.display()
                );
                return ExitCode::from(1);
            }
            files.extend(found);
        }
        if args.recursive {
            files = files
                .into_iter()
                .flat_map(|f| {
                    if f.is_dir() {
                        vsg_rs::config::vhdl_files(&f)
                    } else {
                        vec![f]
                    }
                })
                .collect();
        }
        let mut seen = std::collections::HashSet::new();
        files.retain(|f| seen.insert(f.clone()));
    }
    if !args.stdin && files.is_empty() {
        if args.output_configuration.is_none() {
            let _ = Args::command().print_help();
        }
        return ExitCode::SUCCESS;
    }
    if !args.stdin
        && let Some(missing) = files.iter().find(|f| !f.exists())
    {
        return usage_error(&format!(
            "argument -f/--filename: The file {} does not exist.",
            missing.display()
        ));
    }
    if worker {
        return run_worker(&files, &cfg, &args, options);
    }
    let local = match local_rules_dir {
        Some(dir) => match LocalRules::new(dir, configuration.clone(), args.jobs) {
            Ok(local) => Some(local),
            Err(e) => {
                eprintln!("ERROR: {e}");
                return ExitCode::from(1);
            }
        },
        None => None,
    };
    // With --fix, the local rules fix copies of the files first; vsg-rs continues from them.
    let mut sources: Vec<Option<PathBuf>> = vec![None; files.len()];
    if let Some(local) = local.as_ref().filter(|_| args.fix && !args.stdin) {
        let copies: Vec<(usize, PathBuf)> = files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_file())
            .filter_map(|(i, f)| {
                let source = std::fs::read(f).ok()?;
                Some((i, local.scratch_copy(&format!("in{i}"), f, &source).ok()?))
            })
            .collect();
        let paths: Vec<PathBuf> = copies.iter().map(|(_, c)| c.clone()).collect();
        if let Err(e) = local.run(&paths, true) {
            eprintln!("ERROR: {e}");
            return ExitCode::from(1);
        }
        for (i, copy) in copies {
            sources[i] = Some(copy);
        }
    }
    let workers = if args.stdin {
        Vec::new()
    } else {
        worker_chunks(&files, args.jobs)
    };
    // Several files are read together: the style layer checks uses of package declarations and
    // entity interfaces across files, and the lint layer needs every entity's port modes. Both
    // come from one parse per file, in the workers, so neither costs a pass of its own.
    let mut entities = vsg_rs::analysis::elaborate::Entities::new();
    if !args.stdin && files.len() > 1 {
        let start = std::time::Instant::now();
        let request = |chunk: &[usize]| serde_json::json!({ "index": chunk });
        let index = in_workers(command_line, &workers, request).map_or_else(
            || index_of(&files, style, lint),
            |parts: Vec<Index>| {
                parts.into_iter().fold(Index::default(), |acc, part| Index {
                    project: match (acc.project, part.project) {
                        (Some(a), Some(b)) => Some(Project::merge(a, b)),
                        (a, b) => a.or(b),
                    },
                    entities: match (acc.entities, part.entities) {
                        (Some(a), Some(b)) => Some(merge_entities(a, b)),
                        (a, b) => a.or(b),
                    },
                })
            },
        );
        if let Some(project) = index.project {
            options.project = Some(Arc::new(project));
        }
        entities = index.entities.unwrap_or_default();
        if args.debug {
            eprintln!("DEBUG: project index built in {:.2?}", start.elapsed());
        }
    }
    let rules_checked = cfg.enabled_rule_count();
    let start = std::time::Instant::now();
    let mut stdin_source = Vec::new();
    let mut used_workers = false;
    let mut results: Vec<FileResult> = if args.stdin {
        if let Err(e) = io::stdin().read_to_end(&mut stdin_source) {
            eprintln!("ERROR: stdin: {e}");
            return ExitCode::from(1);
        }
        let mut source = stdin_source.clone();
        if let Some(local) = local.as_ref().filter(|_| args.fix && args.range.is_none()) {
            let name = args
                .stdin_filename
                .clone()
                .unwrap_or_else(|| "stdin.vhd".into());
            let fixed = local
                .scratch_copy("stdin", &name, &source)
                .map_err(|e| e.to_string())
                .and_then(|copy| {
                    local.run(std::slice::from_ref(&copy), true)?;
                    std::fs::read(&copy).map_err(|e| e.to_string())
                });
            match fixed {
                Ok(fixed) => source = fixed,
                Err(e) => {
                    eprintln!("ERROR: {e}");
                    return ExitCode::from(1);
                }
            }
        }
        let (name, cfg) = match &args.stdin_filename {
            Some(path) => (path.display().to_string(), cfg.for_path(path)),
            None => ("stdin".to_owned(), std::borrow::Cow::Borrowed(&cfg)),
        };
        vec![check(&name, source, &cfg, &args, &options)]
    } else {
        let project = serde_json::to_value(options.project.as_deref()).unwrap_or_default();
        let request = |chunk: &[usize]| serde_json::json!({ "check": chunk, "project": project, "sources": sources });
        in_workers(command_line, &workers, request).map_or_else(
            || {
                let all: Vec<usize> = (0..files.len()).collect();
                process(&files, &sources, &all, &cfg, &args, &options)
            },
            |parts: Vec<Vec<FileResult>>| {
                used_workers = true;
                // Back into the order of the inputs.
                let mut slots: Vec<Option<FileResult>> = files.iter().map(|_| None).collect();
                for (chunk, part) in workers.iter().zip(parts) {
                    for (&i, r) in chunk.iter().zip(part) {
                        slots[i] = Some(r);
                    }
                }
                slots.into_iter().flatten().collect()
            },
        )
    };
    if args.debug {
        eprintln!(
            "DEBUG: {} input(s) processed in {:.2?} by {} process(es)",
            results.len(),
            start.elapsed(),
            if used_workers { workers.len() } else { 1 }
        );
    }
    let mut failed = false;
    if let Some(local) = &local
        && let Err(e) = check_local_rules(local, &files, &stdin_source, &args, &mut results)
    {
        eprintln!("ERROR: {e}");
        failed = true;
    }
    // The lint layer: one analysis over the whole file set, after any fixes were written, so its
    // positions match what is now on disk. Style findings stand on their own if it fails.
    // The lint layer may have rules of its own: the base configuration with `--lint_configuration`
    // merged over it, so one file can carry a team's lint policy without touching its style one.
    let lint_cfg = if args.lint_configuration.is_empty() {
        std::borrow::Cow::Borrowed(&cfg)
    } else {
        let mut paths = configuration.clone();
        paths.extend(args.lint_configuration.iter().cloned());
        match Config::load(&paths) {
            Ok(merged) => std::borrow::Cow::Owned(merged),
            Err(e) => {
                eprintln!("ERROR: {e}");
                return ExitCode::from(1);
            }
        }
    };
    if lint {
        // What the lint layer is about: the files named on the command line, or the one buffer
        // `--stdin` supplied. A buffer is analysed as itself, not as whatever is on disk under
        // its name, which is what an editor needs and what `--stdin` always implied.
        let sources: Vec<vsg_rs::analysis::lint::Source> = if args.stdin {
            let path = args
                .stdin_filename
                .clone()
                .unwrap_or_else(|| PathBuf::from("stdin.vhd"));
            vec![vsg_rs::analysis::lint::Source::buffer(
                path,
                stdin_source.clone(),
            )]
        } else {
            files
                .iter()
                .map(vsg_rs::analysis::lint::Source::file)
                .collect()
        };
        // Design checks on our own tree: they need no resolution, so they run per file and
        // survive a file the analyser cannot parse. The port table comes from the index round
        // above; a single input never has one, so it is built here.
        if entities.is_empty() && !args.stdin {
            entities = vsg_rs::analysis::elaborate::entities(&files);
        }
        // Why each file counts as a testbench, for `--debug`. Owned, because a worker sends it.
        let mut kinds: std::collections::BTreeMap<PathBuf, String> =
            std::collections::BTreeMap::new();
        // Which library each file is in, so `testbench_libraries` can name the test ones.
        let of_file = if cfg.testbench_libraries.is_empty() {
            std::collections::BTreeMap::new()
        } else {
            vsg_rs::analysis::lint::libraries_of_files()
        };
        let kind_sources = vsg_rs::analysis::testbench::Kinds {
            patterns: &cfg.testbench_files,
            libraries: &cfg.testbench_libraries,
            of_file: &of_file,
        };
        // The same worker processes the style layer uses: the parser interns token texts in one
        // global lock, so threads in one process contend on every token (see docs/performance.md).
        // The design-wide port table is computed once here and sent along, rather than by each
        // worker over every file.
        let lint_paths: Vec<PathBuf> = if args.lint_configuration.is_empty() {
            Vec::new()
        } else {
            let mut paths = configuration.clone();
            paths.extend(args.lint_configuration.iter().cloned());
            paths
        };
        let workers = worker_chunks(&files, args.jobs);
        let request = |chunk: &[usize]| {
            serde_json::json!({
                "lint": {
                    "indices": chunk,
                    "entities": &entities,
                    "configuration": &lint_paths,
                    "libraries": &of_file,
                }
            })
        };
        let replies: Option<Vec<Vec<PerFile>>> = in_workers(command_line, &workers, request);
        let per_file: Vec<PerFile> = if let Some(replies) = replies {
            {
                // Back in the order the files were given, so a run is reproducible.
                let mut by_index: Vec<Option<PerFile>> = (0..files.len()).map(|_| None).collect();
                for (chunk, reply) in workers.iter().zip(replies) {
                    for (&i, one) in chunk.iter().zip(reply) {
                        by_index[i] = Some(one);
                    }
                }
                by_index.into_iter().flatten().collect()
            }
        } else {
            // One process is enough, or a worker could not be started: do it here.
            let indices: Vec<usize> = (0..sources.len()).collect();
            match lint_files(&sources, &indices, &entities, &lint_cfg, &kind_sources) {
                Ok(per_file) => per_file,
                Err(e) => {
                    eprintln!("ERROR: {e}");
                    return ExitCode::from(1);
                }
            }
        };
        for one in per_file {
            if let Some((file, reason)) = one.kind {
                kinds.insert(file, reason);
            }
            for (file, diagnostic) in one.found {
                if let Some(r) = results.iter_mut().find(|r| Path::new(&r.name) == file) {
                    r.violations.push(diagnostic);
                }
            }
        }
        if args.debug && !kinds.is_empty() {
            eprintln!("DEBUG: {} file(s) treated as testbench:", kinds.len());
            for (file, reason) in &kinds {
                eprintln!("DEBUG:   {} ({reason})", file.display());
            }
        }
        match vsg_rs::analysis::lint::analyse(&sources) {
            Ok(analysis) => {
                let mapped = analysis.mapped;
                let mut held_back = 0usize;
                for f in analysis.findings {
                    if !mapped && !vsg_rs::analysis::lint::needs_no_library_map(f.rule) {
                        held_back += 1;
                        continue;
                    }
                    let Some(r) = results.iter_mut().find(|r| Path::new(&r.name) == f.file) else {
                        continue;
                    };
                    // Picky by default: a lint rule is an error unless the configuration says
                    // otherwise, and disabling it in the configuration switches it off.
                    let file_cfg = lint_cfg.for_kind(if kinds.contains_key(&f.file) {
                        "testbench"
                    } else {
                        "rtl"
                    });
                    let settings = file_cfg.rule_by_id(f.rule);
                    if settings.as_ref().is_some_and(|s| !s.enabled) {
                        continue;
                    }
                    r.violations.push(Diagnostic {
                        line: f.line,
                        column: f.column,
                        rule: f.rule.to_owned(),
                        severity: settings
                            .map_or_else(|| "error".to_owned(), |s| s.severity.to_string()),
                        message: f.message,
                        fixable: false,
                        related: f.related,
                        fix: Vec::new(),
                    });
                }
                for r in &mut results {
                    r.violations.sort_by_key(|d| (d.line, d.column));
                }
                if !mapped {
                    // Say this whether or not anything was held back: a clean report from five
                    // rules must not look like a clean report from all of them.
                    // The same list `--list_rules` and `--explain` read, so the three cannot
                    // disagree about how many rules the lint layer has.
                    let all = lint_rules().count();
                    // The front end's rules, plus the native rules that also read the resolved
                    // project rather than the syntax tree. Counting only the first set would
                    // understate what a missing library map costs.
                    let inactive = vsg_rs::analysis::lint::rules()
                        .filter(|(id, _)| !vsg_rs::analysis::lint::needs_no_library_map(id))
                        .count()
                        + vsg_rs::analysis::calls::RULES.len();
                    let findings = if held_back > 0 {
                        format!(", and {held_back} finding(s) of theirs were held back")
                    } else {
                        String::new()
                    };
                    eprintln!(
                        "WARNING: no vhdl_ls.toml found, so {inactive} of {all} lint rules did \
                         not run{findings}. They need to know which library each file is in; \
                         see https://vsg-rs.readthedocs.io/en/latest/project-setup/"
                    );
                }
                if !analysis.unanalysed.is_empty() {
                    eprintln!(
                        "WARNING: {} of {} file(s) could not be analysed by the lint layer{}",
                        analysis.unanalysed.len(),
                        files.len(),
                        if args.debug {
                            ":"
                        } else {
                            "; run --debug to list them"
                        }
                    );
                    if args.debug {
                        for f in &analysis.unanalysed {
                            eprintln!("DEBUG:   {}", f.display());
                        }
                    }
                }
            }
            Err(e) => eprintln!("WARNING: the lint layer did not run: {e}"),
        }
    }
    // Waivers are applied to everything the run found, whoever produced it (rules, layout or
    // local rules), so one file covers the whole report.
    if let Some(path) = &args.generate_waivers {
        let findings: Vec<(String, String, usize)> = results
            .iter()
            .flat_map(|r| {
                r.violations
                    .iter()
                    .map(|d| (r.name.clone(), d.rule.clone(), d.line))
            })
            .collect();
        let text = crate::waivers::generate(&findings);
        if let Err(e) = std::fs::write(path, &text) {
            eprintln!("ERROR: {}: {e}", path.display());
            return ExitCode::from(1);
        }
        println!(
            "Wrote {} waiver(s) covering {} violation(s) to {}",
            text.lines().filter(|l| l.starts_with("  - rule:")).count(),
            findings.len(),
            path.display()
        );
        return ExitCode::SUCCESS;
    }
    let waivers = match crate::waivers::Waivers::load(&args.waivers) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return ExitCode::from(1);
        }
    };
    let mut waived: Vec<String> = Vec::new();
    if !waivers.is_empty() {
        for r in &mut results {
            let name = r.name.clone();
            r.violations.retain(|d| {
                let Some(w) = waivers.waives(&name, &d.rule, d.line) else {
                    return true;
                };
                let reason = if w.reason.is_empty() {
                    String::new()
                } else {
                    format!(" -- {}", w.reason)
                };
                waived.push(format!("{name}({}){}{reason}", d.line, d.rule));
                false
            });
        }
    }
    let mut report = String::new();
    for (i, r) in results.iter().enumerate() {
        if let Some(e) = &r.error {
            eprintln!("ERROR: {}: {e}", r.name);
            failed = true;
            continue;
        }
        if let Some(output) = &r.output {
            if args.diff {
                let original = if args.stdin {
                    stdin_source.clone()
                } else {
                    std::fs::read(&files[i]).unwrap_or_default()
                };
                let (old, new) = (
                    String::from_utf8_lossy(&original),
                    String::from_utf8_lossy(output),
                );
                let diff = similar::TextDiff::from_lines(old.as_ref(), new.as_ref())
                    .unified_diff()
                    .header(&r.name, &r.name)
                    .to_string();
                let _ = io::stdout().write_all(diff.as_bytes());
            } else if args.stdin {
                let _ = io::stdout().write_all(output);
            } else if let Err(e) = write_fixed(&files[i], output, args.backup) {
                eprintln!("ERROR: {e}");
                failed = true;
                continue;
            }
        }
        // Editors pipe through `--stdin --fix`: printed code means success even if violations
        // remain (VSG itself cannot fix stdin).
        let counted = match &gate {
            // `--fail_on`: only these layers decide the exit code; the rest are still reported.
            Some(layers) => r
                .violations
                .iter()
                .filter(|d| d.severity != "warning" && layers.iter().any(|l| l == kind_of(&d.rule)))
                .count(),
            None => counts(r).0,
        };
        failed |= counted > 0 && !(args.stdin && args.fix);
        match args.output_format {
            OutputFormat::Vsg => vsg_report(&mut report, r, rules_checked),
            OutputFormat::Syntastic => syntastic_report(&mut report, r),
            OutputFormat::Summary => summary_report(&mut report, r, rules_checked),
        }
    }
    if !waived.is_empty() {
        use std::fmt::Write as _;
        if args.show_waived {
            for line in &waived {
                let _ = writeln!(report, "WAIVED: {line}");
            }
        }
        let _ = writeln!(report, "{} violation(s) waived", waived.len());
    }
    if args.statistics {
        report += &statistics_report(&results, &cfg);
    }
    // With `--stdin --fix` or `--diff`, stdout carries the source or diff; the report goes to
    // stderr.
    if (args.stdin && args.fix) || args.diff {
        eprint!("{report}");
    } else {
        print!("{report}");
    }
    let outputs: [(&Option<PathBuf>, Render); 5] = [
        (&args.json, json_report),
        (&args.sarif, sarif_report),
        (&args.junit, junit_report),
        (&args.quality_report, quality_report),
        (&args.sonarqube, sonar_report),
    ];
    for (path, render) in outputs {
        if let Some(path) = path
            && let Err(e) = write_file(path, &(render(&results) + "\n"))
        {
            eprintln!("ERROR: {e}");
            failed = true;
        }
    }
    let _ = io::stdout().flush();
    ExitCode::from(u8::from(failed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(violations: Vec<(&str, usize, &str)>) -> FileResult {
        FileResult {
            name: "a.vhd".into(),
            violations: violations
                .into_iter()
                .map(|(rule, line, severity)| Diagnostic {
                    line,
                    column: 1,
                    rule: rule.into(),
                    severity: severity.into(),
                    message: "Add *entity* keyword".into(),
                    fixable: false,
                    related: Vec::new(),
                    fix: Vec::new(),
                })
                .collect(),
            error: None,
            output: None,
        }
    }

    #[test]
    fn vsg_table_layout() {
        let mut out = String::new();
        vsg_report(
            &mut out,
            &result(vec![("entity_019", 3, "error"), ("port_014", 2, "error")]),
            177,
        );
        let expected = "\
================================================================================
File:  a.vhd
================================================================================
Phase 1 of 1... Reporting
Total Rules Checked: 177
Total Violations:    2
  Error   :     2
  Warning :     0
-------------+------------+------------+--------------------------------------
  Rule       |  severity  |  line(s)   | Solution
-------------+------------+------------+--------------------------------------
  port_014   | Error      |          2 | Add *entity* keyword
  entity_019 | Error      |          3 | Add *entity* keyword
-------------+------------+------------+--------------------------------------
NOTE: Refer to online documentation at https://vhdl-style-guide.readthedocs.io/en/latest/index.html for more information.
";
        assert_eq!(out, expected);
        let mut clean = String::new();
        vsg_report(&mut clean, &result(vec![]), 889);
        assert!(clean.ends_with("  Warning :     0\n\n"));
    }

    #[test]
    fn short_options() {
        let args = |a: &[&str]| normalize(a.iter().map(|s| (*s).to_owned()));
        assert_eq!(
            args(&["vsg-rs", "-fp", "3", "-of", "summary", "-f", "x"]),
            [
                "vsg-rs",
                "--fix_phase",
                "3",
                "--output_format",
                "summary",
                "-f",
                "x"
            ]
        );
        let parsed = Args::try_parse_from(args(&[
            "vsg-rs", "-f", "a.vhd", "b.vhd", "-js", "o.json", "-ap", "--fix",
        ]))
        .unwrap();
        assert_eq!(parsed.filename.len(), 2);
        assert!(parsed.fix && parsed.all_phases && parsed.json.is_some());
    }

    #[test]
    fn timestamp_format() {
        let t = timestamp();
        assert_eq!(t.len(), 19);
        assert_eq!(&t[4..5], "-");
        assert_eq!(&t[10..11], "T");
    }
}

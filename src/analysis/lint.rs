//! The lint layer (`--check lint`): rules that need names to be resolved.
//!
//! "Lint" in the sense `lint(1)` and every EDA lint tool mean it — code that is legal but
//! probably wrong — as opposed to the `style` layer, which is VSG's style guide. None of this is
//! VSG: these rules have no VSG counterpart and carry ids of their own, so a default run still
//! reports exactly what VSG reports.
//!
//! `vhdl_lang` does the analysis. It keeps its own syntax tree, so this is a second parse in a
//! second phase rather than anything the formatter shares, and it runs once per invocation over
//! the whole file set, because resolution needs the library rather than one file.
//!
//! Only `vhdl_lang`'s two linters are reported, as `lint_001` to `lint_004`. Its
//! analysis diagnostics (unresolved names, type mismatches) are deliberately dropped for now:
//! they are worth more but they flood the report when the library mapping is incomplete, which
//! by default it is.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use vhdl_lang::{Config, Diagnostic, MessageHandler, Project, SrcPos};

include!(concat!(env!("OUT_DIR"), "/vhdl_libraries.rs"));

/// One finding, in the shape the command line reports.
/// Another place that is part of the same finding: the other driver, the next signal in a
/// cycle, the declaration a name resolves to. Structured rather than written into the message,
/// so the console, SARIF and any future consumer can each present it their own way.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Related {
    pub file: PathBuf,
    /// One-based, as every report format wants it.
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug)]
pub struct Finding {
    pub file: PathBuf,
    pub rule: &'static str,
    /// One-based, as every report format wants it.
    pub line: usize,
    pub column: usize,
    pub message: String,
    /// The other places this finding is about. Empty for most rules.
    pub related: Vec<Related>,
}

pub struct Analysis {
    pub findings: Vec<Finding>,
    /// Inputs `vhdl_lang` could not parse, which keep their style findings and nothing else.
    pub unanalysed: Vec<PathBuf>,
    /// Whether the project told us what its libraries are (`vhdl_ls.toml`).
    pub mapped: bool,
}

/// Rules that only need the file itself, and so mean something even when the run does not know
/// what the project's libraries are.
///
/// Without a library map every name from another library is unresolved, and an unresolved name
/// makes the rules that depend on resolution produce nonsense: on 50 `VUnit` files, 9968
/// `lint_100` findings and 623 knock-on `lint_302`. Reporting those by default would make the
/// first run useless, so they wait until the project says where its libraries are.
pub fn needs_no_library_map(rule: &str) -> bool {
    matches!(rule, "lint_001" | "lint_002" | "lint_003")
}

/// Every `vhdl_lang` diagnostic this layer reports, as (its error code, our rule id, what it
/// means). The code names come from `ErrorCode`'s `Debug` form, because the published crate does
/// not export the enum itself.
///
/// `SyntaxError` and `Internal` are not here: vsg-rs reports parse failures itself, and an
/// internal error of the analyser is not a finding about the code.
pub static CODES: &[(&str, &str, &str)] = &[
    // Advisory lints
    (
        "MissingInSensitivityList",
        "lint_001",
        "A signal read by a combinational process is missing from its sensitivity list.",
    ),
    (
        "SuperfluousInSensitivityList",
        "lint_002",
        "A signal in a sensitivity list is not read by the process.",
    ),
    (
        "DisallowedInSensitivityList",
        "lint_003",
        "An item that may not appear in a sensitivity list.",
    ),
    ("Unused", "lint_004", "A declaration is never used."),
    (
        "UnnecessaryWorkLibrary",
        "lint_005",
        "`library work` is implicit and does not need declaring.",
    ),
    (
        "UnassociatedContext",
        "lint_006",
        "A context clause is not attached to any design unit.",
    ),
    // Names and declarations
    (
        "Unresolved",
        "lint_100",
        "A name cannot be resolved to any declaration.",
    ),
    (
        "Duplicate",
        "lint_101",
        "A declaration hides or repeats another.",
    ),
    (
        "DeclaredBefore",
        "lint_102",
        "A declaration is used before it is declared.",
    ),
    (
        "MissingDeferredDeclaration",
        "lint_103",
        "A deferred constant has no full declaration.",
    ),
    (
        "MissingFullTypeDeclaration",
        "lint_104",
        "An incomplete type has no full declaration.",
    ),
    (
        "MissingProtectedBodyType",
        "lint_105",
        "A protected type has no body.",
    ),
    (
        "IllegalDeferredConstant",
        "lint_106",
        "A deferred constant where none is allowed.",
    ),
    (
        "DeclarationNotAllowed",
        "lint_107",
        "A declaration that this region does not allow.",
    ),
    (
        "ConflictingUseClause",
        "lint_108",
        "Two use clauses make the same name visible.",
    ),
    (
        "CircularDependency",
        "lint_109",
        "Design units depend on each other in a cycle.",
    ),
    (
        "ConfigNotInSameLibrary",
        "lint_110",
        "A configuration is not in the library of the entity it configures.",
    ),
    // Types and expressions
    (
        "TypeMismatch",
        "lint_200",
        "An expression does not have the expected type.",
    ),
    (
        "DimensionMismatch",
        "lint_201",
        "Array dimensions do not match.",
    ),
    (
        "NoImplicitConversion",
        "lint_202",
        "The types have no implicit conversion between them.",
    ),
    (
        "InvalidLiteral",
        "lint_203",
        "A literal is not valid for its type.",
    ),
    (
        "ExpectedSubAggregate",
        "lint_204",
        "A sub-aggregate was expected.",
    ),
    (
        "AmbiguousExpression",
        "lint_205",
        "An expression has more than one possible type.",
    ),
    (
        "NonScalarInRange",
        "lint_206",
        "A range bound is not scalar.",
    ),
    (
        "TooManyConstraints",
        "lint_207",
        "More constraints than the type has dimensions.",
    ),
    (
        "TooFewConstraints",
        "lint_208",
        "Fewer constraints than the type needs.",
    ),
    (
        "IllegalConstraint",
        "lint_209",
        "A constraint that this type does not accept.",
    ),
    (
        "MismatchedKinds",
        "lint_210",
        "A name is used as the wrong kind of thing.",
    ),
    (
        "MismatchedObjectClass",
        "lint_211",
        "A signal, variable or constant used where another was expected.",
    ),
    (
        "MismatchedEntityClass",
        "lint_212",
        "An entity class that does not match the declaration.",
    ),
    (
        "CannotBePrefixed",
        "lint_213",
        "A name that cannot take a prefix.",
    ),
    (
        "IllegalAttribute",
        "lint_214",
        "An attribute that does not apply here.",
    ),
    (
        "MisplacedAttributeSpec",
        "lint_215",
        "An attribute specification in the wrong place.",
    ),
    // Subprograms and calls
    (
        "AmbiguousCall",
        "lint_300",
        "A call matches more than one subprogram.",
    ),
    (
        "InvalidCall",
        "lint_301",
        "A call that cannot be made here.",
    ),
    (
        "TooManyArguments",
        "lint_302",
        "More arguments than the subprogram takes.",
    ),
    (
        "NamedBeforePositional",
        "lint_303",
        "A positional association after a named one.",
    ),
    (
        "SignatureMismatch",
        "lint_304",
        "A signature does not match the subprogram.",
    ),
    (
        "IllegalSignature",
        "lint_305",
        "A signature where none is allowed.",
    ),
    (
        "SignatureRequired",
        "lint_306",
        "A signature is needed to disambiguate.",
    ),
    (
        "NoOverloadedWithSignature",
        "lint_307",
        "No overload matches the signature.",
    ),
    (
        "UnexpectedSignature",
        "lint_308",
        "A signature on something that cannot have one.",
    ),
    (
        "InvalidOperatorSymbol",
        "lint_309",
        "An operator symbol that is not an operator.",
    ),
    ("VoidReturn", "lint_310", "A function returns nothing."),
    ("NonVoidReturn", "lint_311", "A procedure returns a value."),
    (
        "IllegalReturn",
        "lint_312",
        "A return statement where none is allowed.",
    ),
    (
        "MismatchedSubprogramInstantiation",
        "lint_313",
        "A subprogram instantiation does not match its target.",
    ),
    (
        "AmbiguousInstantiation",
        "lint_314",
        "An instantiation matches more than one target.",
    ),
    // Ports, generics and associations
    (
        "Unassociated",
        "lint_400",
        "A port or generic has no association and no default.",
    ),
    (
        "AlreadyAssociated",
        "lint_401",
        "A formal is associated more than once.",
    ),
    (
        "InvalidFormal",
        "lint_402",
        "A formal that the interface does not have.",
    ),
    (
        "InvalidFormalConversion",
        "lint_403",
        "A conversion on a formal that is not allowed.",
    ),
    (
        "InterfaceModeMismatch",
        "lint_404",
        "An actual does not match the mode of its formal.",
    ),
    // Statements
    (
        "ExitOutsideLoop",
        "lint_500",
        "An exit statement outside a loop.",
    ),
    (
        "NextOutsideLoop",
        "lint_501",
        "A next statement outside a loop.",
    ),
    (
        "InvalidLoopLabel",
        "lint_502",
        "A loop label that does not name an enclosing loop.",
    ),
];

fn rule_of(code: &str) -> Option<(&'static str, &'static str)> {
    CODES
        .iter()
        .find(|(name, _, _)| *name == code)
        .map(|(_, rule, description)| (*rule, *description))
}

/// Every rule this layer can report, for `--list_rules` and the configuration.
pub fn rules() -> impl Iterator<Item = (&'static str, &'static str)> {
    CODES
        .iter()
        .map(|(_, rule, description)| (*rule, *description))
}

/// Silence `vhdl_lang`'s own progress and configuration messages; ours is the report that counts.
struct Quiet;

impl MessageHandler for Quiet {
    fn push(&mut self, _: vhdl_lang::Message) {}
}

/// Write the embedded `ieee` and `std` sources somewhere on disk, because a library mapping is
/// file based. Written once per version and reused; the contents cannot change within a build.
fn libraries() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    let root = base
        .join("vsg-rs")
        .join(format!("vhdl_libraries-{}", env!("CARGO_PKG_VERSION")));
    for (name, text) in VHDL_LIBRARIES {
        let path = root.join(name);
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(root)
}

/// The configuration `vhdl_lang` analyses with: the project's own `vhdl_ls.toml` when it has
/// one, otherwise every input in `work`, plus the embedded standard libraries either way.
fn configuration(files: &[PathBuf], project_config: Option<&Path>) -> Result<Config, String> {
    let root = libraries()?;
    let mut config = Config::from_str(
        &format!(
            "[libraries]\nstd.files = ['{0}/std/*.vhd']\nstd.is_third_party = true\n\
             ieee.files = ['{0}/ieee2008/*.vhdl', '{0}/synopsys/*.vhdl', '{0}/vital2000/*.vhdl']\n\
             ieee.is_third_party = true\n",
            root.to_string_lossy().replace('\\', "/")
        ),
        Path::new("."),
    )?;
    if let Some(path) = project_config {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let parent = path.parent().unwrap_or(Path::new("."));
        let project = Config::from_str(&text, parent)?;
        config.append(&project, &mut Quiet);
    } else {
        {
            // No library map: everything the run was given goes into one library. It cannot be
            // called `work` (a reserved name that means "the current library"), so every file
            // sees every other, which is the best guess without a project file.
            let list: Vec<String> = files
                .iter()
                .map(|f| format!("'{}'", f.to_string_lossy().replace('\\', "/")))
                .collect();
            let work = Config::from_str(
                &format!("[libraries]\ndefaultlib.files = [{}]\n", list.join(", ")),
                Path::new("."),
            )?;
            config.append(&work, &mut Quiet);
        }
    }
    Ok(config)
}

fn position(pos: &SrcPos) -> (usize, usize) {
    (
        pos.range.start.line as usize + 1,
        pos.range.start.character as usize + 1,
    )
}

/// Fold the related positions into the message: a sensitivity-list finding is much easier to act
/// on when it says where the signal is read, and a violation has one span.
fn message(d: &Diagnostic) -> String {
    let mut text = d.message.clone();
    let extra: Vec<String> = d
        .related
        .iter()
        .map(|(pos, note)| format!("{note} at line {}", position(pos).0))
        .collect();
    if !extra.is_empty() {
        let _ = write!(text, " ({})", extra.join(", "));
    }
    text
}

/// The project's own library map, if it has one where `vhdl_ls` would look for it.
fn project_config() -> Option<PathBuf> {
    let path = PathBuf::from("vhdl_ls.toml");
    path.is_file().then_some(path)
}

/// Which libraries each file belongs to, from the project's `vhdl_ls.toml`. Empty when the
/// project has no library map, which is also when `vsg_rs: testbench_libraries` cannot be used.
pub fn libraries_of_files() -> BTreeMap<PathBuf, Vec<String>> {
    let mut out: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    let Some(path) = project_config() else {
        return out;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return out;
    };
    let parent = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let Ok(config) = Config::from_str(&text, &parent) else {
        return out;
    };
    for library in config.iter_libraries() {
        let name = library.name().to_ascii_lowercase();
        for file in library.file_names(&mut Quiet) {
            let file = std::fs::canonicalize(&file).unwrap_or(file);
            out.entry(file).or_default().push(name.clone());
        }
    }
    out
}

/// A file to analyse: what is on disk, or the buffer an editor currently holds for it.
///
/// The path is always meaningful even when the text is not on disk: it decides which library the
/// file belongs to and what a finding is labelled with.
#[derive(Debug, Clone)]
pub struct Source {
    pub path: PathBuf,
    /// The contents to analyse. `None` reads the file.
    pub text: Option<Vec<u8>>,
}

impl Source {
    /// A file, read from disk.
    #[must_use]
    pub fn file(path: impl Into<PathBuf>) -> Source {
        Source {
            path: path.into(),
            text: None,
        }
    }

    /// A buffer standing in for a file that may never have been saved.
    #[must_use]
    pub fn buffer(path: impl Into<PathBuf>, text: Vec<u8>) -> Source {
        Source {
            path: path.into(),
            text: Some(text),
        }
    }
}

/// An analysed project, kept so it can be asked again.
///
/// Building one parses every file the library map names, plus the `ieee` and `std` libraries that
/// ship inside the binary. That is most of what a single analysis costs, and it is the same work
/// every time. An editor asks after every keystroke, so it keeps the project and replaces only
/// the buffer that changed.
pub struct Analyser {
    project: Project,
    mapped: bool,
}

impl Analyser {
    /// Read the project's library map, or treat the given files as one library when it has none.
    ///
    /// # Errors
    /// If the configuration names something that cannot be read.
    pub fn new(sources: &[Source]) -> Result<Analyser, String> {
        let files: Vec<PathBuf> = sources.iter().map(|s| s.path.clone()).collect();
        let config = configuration(&files, project_config().as_deref())?;
        let mut project = Project::from_config(config, &mut Quiet);
        project.enable_all_linters();
        Ok(Analyser {
            project,
            mapped: project_config().is_some(),
        })
    }

    /// The subprograms that can reach themselves, from the resolved call graph.
    ///
    /// Separate from [`Analyser::analyse`] because it walks the whole design rather than the
    /// files that changed, which is the wrong shape for an editor asking after every keystroke.
    /// Call it after `analyse`; before that nothing is resolved and the answer is empty.
    #[must_use]
    pub fn recursion(&self, sources: &[Source]) -> Vec<Finding> {
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let wanted: BTreeMap<PathBuf, PathBuf> = sources
            .iter()
            .map(|s| (canonical(&s.path), s.path.clone()))
            .collect();
        super::calls::recursion(&self.project, &wanted)
    }

    /// Analyse, with each source's buffer standing in for its file where it has one.
    pub fn analyse(&mut self, sources: &[Source]) -> Analysis {
        // An editor's buffer replaces the file it stands for. `update_source` re-parses in place
        // and registers a path the project has not seen, which is how an unsaved file is
        // analysed at all.
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        for source in sources.iter().filter(|s| s.text.is_some()) {
            let Some(text) = source.text.as_deref() else {
                continue;
            };
            // VHDL may be Latin-1; anything that is not valid UTF-8 is replaced rather than
            // refused, so a buffer is always analysable even while it is being typed.
            let text = String::from_utf8_lossy(text);
            // The project knows a file by the path its library map resolved to, which is the
            // symlink-resolved one — every macOS temporary directory is reached through a
            // symlink, and an editor sends the path the user opened. Updating under the other
            // spelling registers a source belonging to no library, so the buffer is never
            // analysed and the file on disk silently answers in its place.
            let path = if self.project.get_source(&source.path).is_some() {
                source.path.clone()
            } else {
                canonical(&source.path)
            };
            self.project
                .update_source(&vhdl_lang::Source::inline(&path, &text));
        }

        // The inputs, by canonical path, so findings can be mapped back to the name the run was
        // given and findings in other files (the standard libraries) can be dropped.
        let wanted: BTreeMap<PathBuf, PathBuf> = sources
            .iter()
            .map(|s| (canonical(&s.path), s.path.clone()))
            .collect();
        // `vhdl_lang` parses independently of `vhdl_syntax`, so a file can format but not analyse.
        let unanalysed: Vec<PathBuf> = wanted
            .iter()
            .filter(|(canonical, given)| {
                self.project.get_source(canonical).is_none()
                    && self.project.get_source(given).is_none()
            })
            .map(|(_, given)| given.clone())
            .collect();

        let mut findings = Vec::new();
        for d in self.project.analyse() {
            let Some((rule, _)) = rule_of(&format!("{:?}", d.code)) else {
                continue;
            };
            let Some(given) = wanted.get(&canonical(d.pos.source.file_name())) else {
                continue;
            };
            let (line, column) = position(&d.pos);
            findings.push(Finding {
                file: given.clone(),
                rule,
                line,
                column,
                message: message(&d),
                related: Vec::new(),
            });
        }
        findings.sort_by(|a, b| {
            (&a.file, a.line, a.column, a.rule).cmp(&(&b.file, b.line, b.column, b.rule))
        });
        Analysis {
            findings,
            unanalysed,
            mapped: self.mapped,
        }
    }
}

/// Analyse once. A caller that asks repeatedly should keep an [`Analyser`] instead.
///
/// # Errors
/// If the project configuration cannot be read.
pub fn analyse(sources: &[Source]) -> Result<Analysis, String> {
    let mut analyser = Analyser::new(sources)?;
    let mut analysis = analyser.analyse(sources);
    // The call graph needs the same resolved project, and only a whole-run caller can afford
    // the walk, so it is added here rather than inside `analyse`.
    analysis.findings.extend(analyser.recursion(sources));
    analysis.findings.sort_by(|a, b| {
        (&a.file, a.line, a.column, a.rule).cmp(&(&b.file, b.line, b.column, b.rule))
    });
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same source, analysed as a buffer for a path whose file says something else.
    fn analyse_buffer(text: &str) -> Vec<(&'static str, usize)> {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("dut.vhd");
        std::fs::write(&path, "entity other is\nend entity other;\n").expect("write");
        let out =
            analyse(&[Source::buffer(&path, text.as_bytes().to_vec())]).expect("analysis runs");
        out.findings.iter().map(|f| (f.rule, f.line)).collect()
    }

    #[test]
    fn a_buffer_is_analysed_instead_of_the_file_it_stands_for() {
        let text = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n\n  \
                    signal spare : bit;\n\nbegin\n\nend architecture rtl;\n";
        // The file on disk is a different entity entirely; the buffer is what gets analysed.
        assert_eq!(analyse_buffer(text), analyse_source(text));
        assert!(
            !analyse_buffer(text).is_empty(),
            "the spare signal is unused"
        );
    }

    fn analyse_source(text: &str) -> Vec<(&'static str, usize)> {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("dut.vhd");
        std::fs::write(&path, text).expect("write");
        let out = analyse(&[Source::file(&path)]).expect("analysis runs");
        assert!(out.unanalysed.is_empty(), "{:?}", out.unanalysed);
        out.findings.iter().map(|f| (f.rule, f.line)).collect()
    }

    #[test]
    fn finds_a_missing_and_a_superfluous_sensitivity_list_signal() {
        let found = analyse_source(
            "entity dut is\n\
             end entity;\n\
             architecture rtl of dut is\n\
             \x20 signal a, b, c : bit;\n\
             begin\n\
             \x20 p : process (a, c) is\n\
             \x20 begin\n\
             \x20   c <= a and b;\n\
             \x20 end process;\n\
             end architecture;\n",
        );
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_001"),
            "expected a missing signal, got {found:?}"
        );
        assert!(
            found.iter().any(|(rule, _)| *rule == "lint_002"),
            "expected a superfluous signal, got {found:?}"
        );
    }

    #[test]
    fn a_clocked_process_is_not_checked() {
        let found = analyse_source(
            "entity dut is\n\
             end entity;\n\
             architecture rtl of dut is\n\
             \x20 signal clk, d, q : bit;\n\
             begin\n\
             \x20 p : process (clk) is\n\
             \x20 begin\n\
             \x20   if clk'event and clk = '1' then\n\
             \x20     q <= d;\n\
             \x20   end if;\n\
             \x20 end process;\n\
             end architecture;\n",
        );
        assert!(
            !found.iter().any(|(rule, _)| *rule == "lint_001"),
            "a clocked process needs no data signals in its list, got {found:?}"
        );
    }
}

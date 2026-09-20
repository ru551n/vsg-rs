//! Configuration: VSG-compatible files resolved into the settings the formatter and the rules
//! consume.
//!
//! Pipeline: raw YAML/JSON document → validation (unknown or unsupported keys become warnings)
//! → VSG compatibility mapping → [`Config`]. Formatter and rules only read the resolved form.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use yaml_serde::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordCase {
    Lower,
    Upper,
    Preserve,
}

/// Formatting policy. Everything here influences canonical output.
///
/// `#[non_exhaustive]`: new options are added here on most releases, so build one from
/// [`FormatConfig::default`] (or [`crate::Config`]) and assign the fields you care about
/// rather than writing a struct literal.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FormatConfig {
    /// Target line width in display columns (see `docs/line-folding.md`).
    pub width: usize,
    /// Columns per indentation level.
    pub indent: usize,
    /// Indent with tabs and align with spaces (VSG `indent_style: smart_tabs`).
    pub tabs: bool,
    pub keyword_case: KeywordCase,
    /// Keywords (lower case) whose case differs from `keyword_case` everywhere.
    pub keyword_case_overrides: BTreeMap<String, KeywordCase>,
    /// Keywords whose case differs inside particular constructs (per-keyword VSG rules).
    pub keyword_case_in:
        std::collections::HashMap<String, Vec<(vhdl_syntax::syntax::NodeKind, KeywordCase)>>,
    /// Output line ending; `None` keeps the line ending of the input.
    pub line_ending: Option<LineEnding>,
    /// Blank-line policy (VSG `blank_line` rules).
    pub blank: crate::blank::BlankSettings,
    /// Alignment policy (VSG `alignment` rules).
    pub align: crate::align::AlignSettings,
    /// Indentation of construct parts (VSG `indent.tokens`).
    pub indent_policy: crate::indent::IndentPolicy,
    /// Spaces between particular tokens (VSG `number_of_spaces`), where configured unlike VSG's
    /// default.
    pub spacing: Vec<crate::layout::SpacingRule>,
    /// Spaces before and after the port modes `in`, `out` and `inout` (VSG `port_007` to
    /// `port_009`).
    pub mode_spacing: [(usize, usize); 3],
    /// Generic clause, port clause, generic map and port map whose `)` stays on the last
    /// element's line (VSG `action: same_line`).
    pub close_paren_same_line: Vec<vhdl_syntax::syntax::NodeKind>,
    /// Continuation lines are indented instead of aligned (VSG `align_left`/`align_paren`).
    pub indent_continuations: bool,
    /// Spaces before a trailing comment (VSG `comment_004`), where alignment does not decide.
    pub comment_spaces: usize,
    /// Re-wrap comment paragraphs to `width` (vsg-rs extension `vsg_rs: reflow_comments`).
    pub reflow_comments: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl Default for FormatConfig {
    fn default() -> Self {
        FormatConfig {
            width: 120,
            indent: 2,
            tabs: false,
            keyword_case: KeywordCase::Lower,
            keyword_case_overrides: BTreeMap::new(),
            keyword_case_in: std::collections::HashMap::new(),
            line_ending: None,
            blank: crate::blank::BlankSettings::default(),
            align: crate::align::AlignSettings::default(),
            indent_policy: crate::indent::IndentPolicy::default(),
            spacing: Vec::new(),
            mode_spacing: [(1, 4), (1, 3), (1, 1)],
            close_paren_same_line: Vec::new(),
            indent_continuations: false,
            comment_spaces: 1,
            reflow_comments: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Warning => "warning",
            Severity::Error => "error",
        })
    }
}

/// Settings of one rule as written in a configuration file (`None`: not set at this level).
#[derive(Debug, Clone, Default)]
struct RuleLayer {
    disable: Option<bool>,
    severity: Option<Severity>,
    fixable: Option<bool>,
    options: BTreeMap<String, Value>,
}

/// Effective settings of one rule.
#[derive(Debug, Clone)]
pub struct RuleSettings {
    pub enabled: bool,
    pub severity: Severity,
    /// Whether `vsg-rs fix` applies the rule's fixes: VSG's default for the rule unless the
    /// configuration sets `fixable`.
    pub fixable: bool,
    /// The configuration sets `fixable` (as opposed to VSG's default).
    pub fixable_configured: bool,
    options: BTreeMap<String, Value>,
}

impl RuleSettings {
    pub fn option_str(&self, key: &str) -> Option<&str> {
        self.options.get(key).and_then(Value::as_str)
    }

    pub fn option_usize(&self, key: &str) -> Option<usize> {
        self.options
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
    }

    pub fn has_option(&self, key: &str) -> bool {
        self.options.contains_key(key)
    }

    pub fn option_bool(&self, key: &str) -> Option<bool> {
        self.options.get(key).and_then(Value::as_bool)
    }

    /// A list of strings; a single string counts as a one-element list.
    pub fn option_list(&self, key: &str) -> Vec<&str> {
        match self.options.get(key) {
            Some(Value::Sequence(items)) => items.iter().filter_map(Value::as_str).collect(),
            Some(Value::String(s)) => vec![s.as_str()],
            _ => Vec::new(),
        }
    }
}

/// Resolved configuration.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub format: FormatConfig,
    global: RuleLayer,
    groups: BTreeMap<String, RuleLayer>,
    rules: BTreeMap<String, RuleLayer>,
    /// `pragma.patterns` from the configuration: (open, close) regular expressions.
    pragma_patterns: Option<(Vec<String>, Vec<String>)>,
    /// The `indent` and `pragma` blocks as written (for the effective configuration).
    raw_indent: Option<serde_json::Value>,
    raw_pragma: Option<serde_json::Value>,
    /// `file_rules`: (path pattern, `rule` block) applied to matching files.
    file_rules: Vec<(String, Value)>,
    /// `vsg_rs: testbench_files`: globs naming the files that are testbenches, not hardware.
    pub testbench_files: Vec<String>,
    /// `vsg_rs: testbench_libraries`: libraries (from `vhdl_ls.toml`) that hold testbenches.
    pub testbench_libraries: Vec<String>,
    /// `vsg_rs: synchronizers`: entity names (globs) that make a clock domain crossing safe.
    pub synchronizers: Vec<String>,
    /// `vsg_rs: rtl` / `vsg_rs: testbench`: a `rule` block for each kind of file.
    kind_rules: BTreeMap<String, Value>,
    /// `file_list`: (path or glob pattern, the configuration file that lists it).
    pub file_list: Vec<(String, PathBuf)>,
    /// `local_rules`: the directory of VSG rule plugins.
    pub local_rules: Option<PathBuf>,
    /// Configured rule ids that vsg-rs does not know (possibly local rules).
    unknown_rules: Vec<String>,
    /// Problems found while loading that did not prevent loading.
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct ConfigError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ConfigError {}

const fn formatter_rule(id: &'static str) -> crate::rules::RuleInfo {
    crate::rules::RuleInfo::formatter(
        id,
        &["blank_line"],
        true,
        "Blank line policy (applied by the formatter).",
    )
}

static WHITESPACE_200: crate::rules::RuleInfo = crate::rules::RuleInfo {
    groups: &["whitespace"],
    ..formatter_rule("whitespace_200")
};

static PRAGMA_RULES: [crate::rules::RuleInfo; 4] = [
    formatter_rule("pragma_400"),
    formatter_rule("pragma_401"),
    formatter_rule("pragma_402"),
    formatter_rule("pragma_403"),
];

/// File names looked up by [`discover`], in order of preference.
pub const CONFIG_FILE_NAMES: [&str; 4] =
    ["vsg-rs.yaml", ".vsg-rs.yaml", "vsg-rs.json", ".vsg-rs.json"];

/// The nearest configuration file in `dir` or its ancestors.
pub fn discover(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .flat_map(|d| CONFIG_FILE_NAMES.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file())
}

/// The files matching a `file_list` pattern (`*`, `?`, `**`), relative to the working
/// directory, in sorted order. A pattern without wildcards is returned if the file exists.
pub fn expand_pattern(pattern: &str) -> Vec<PathBuf> {
    let pattern = &expand_vars(pattern);
    if !pattern.contains(['*', '?']) {
        return Some(PathBuf::from(pattern))
            .filter(|p| p.exists())
            .into_iter()
            .collect();
    }
    let fixed: Vec<&str> = pattern
        .split('/')
        .take_while(|c| !c.contains(['*', '?']))
        .collect();
    let base = if fixed.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(fixed.join("/"))
    };
    walk(&base)
        .into_iter()
        .filter(|p| {
            let text = p.to_string_lossy().replace('\\', "/");
            glob(pattern.as_bytes(), text.trim_start_matches("./").as_bytes())
        })
        .map(|p| p.strip_prefix(".").map(Path::to_path_buf).unwrap_or(p))
        .collect()
}

/// A path with environment variables and a leading `~` expanded, as VSG does.
pub fn expand_path(text: &str) -> PathBuf {
    let text = expand_vars(text);
    match (text.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(text),
    }
}

/// `$NAME` and `${NAME}` replaced by environment variables; unknown ones are kept.
fn expand_vars(text: &str) -> String {
    static VAR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    VAR.get_or_init(|| regex::Regex::new(r"\$(?:\{(\w+)\}|(\w+))").expect("valid regex"))
        .replace_all(text, |c: &regex::Captures| {
            let name = c.get(1).or_else(|| c.get(2)).map_or("", |m| m.as_str());
            std::env::var(name).unwrap_or_else(|_| c[0].to_owned())
        })
        .into_owned()
}

/// The VHDL files (`.vhd`, `.vhdl`) in `dir` and its subdirectories, in sorted order. Hidden
/// files and directories are skipped.
pub fn vhdl_files(dir: &Path) -> Vec<PathBuf> {
    walk(dir)
        .into_iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("vhd") || e.eq_ignore_ascii_case("vhdl"))
        })
        .collect()
}

/// The files below `dir`, skipping hidden entries.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(_) => out.push(path),
                Err(_) => {}
            }
        }
    }
    out.sort();
    out
}

impl Config {
    /// Load and merge configuration files; later files override earlier ones.
    pub fn load(paths: &[PathBuf]) -> Result<Config, ConfigError> {
        let mut cfg = Config::default();
        for path in paths {
            let error = |message: String| ConfigError {
                path: path.clone(),
                message,
            };
            let text = std::fs::read_to_string(path).map_err(|e| error(e.to_string()))?;
            // JSON is a subset of YAML, so one parser handles both formats.
            let doc: Value = yaml_serde::from_str(&text).map_err(|e| error(e.to_string()))?;
            let listed = cfg.file_list.len();
            cfg.merge(&doc).map_err(error)?;
            for entry in &mut cfg.file_list[listed..] {
                entry.1.clone_from(path);
            }
        }
        cfg.resolve_format();
        Ok(cfg)
    }

    /// Load configuration from a YAML or JSON string.
    pub fn parse(text: &str) -> Result<Config, String> {
        let doc: Value = yaml_serde::from_str(text).map_err(|e| e.to_string())?;
        let mut cfg = Config::default();
        cfg.merge(&doc)?;
        cfg.resolve_format();
        Ok(cfg)
    }

    fn merge(&mut self, doc: &Value) -> Result<(), String> {
        let Some(map) = doc.as_mapping() else {
            if doc.is_null() {
                return Ok(());
            }
            return Err("expected a mapping at the top level".into());
        };
        for (key, value) in map {
            let key = key.as_str().unwrap_or_default();
            match key {
                "rule" => self.merge_rules(value)?,
                "linesep" => {
                    self.format.line_ending = match value.as_str() {
                        Some("\n") => Some(LineEnding::Lf),
                        Some("\r\n") => Some(LineEnding::CrLf),
                        _ => return Err(format!("linesep: unsupported value {value:?}")),
                    };
                }
                "file_list" => self.merge_file_list(value)?,
                "file_rules" => self.merge_file_rules(value)?,
                "pragma" => {
                    self.merge_pragma(value)?;
                    merge_raw(&mut self.raw_pragma, value);
                }
                "indent" => merge_raw(&mut self.raw_indent, value),
                "vsg_rs" => self.merge_extensions(value)?,
                "local_rules" => {
                    let dir = value.as_str().ok_or("`local_rules` must be a directory")?;
                    self.local_rules = Some(expand_path(dir));
                }
                _ => self.warn(&format!("unknown top-level key `{key}` ignored")),
            }
        }
        Ok(())
    }

    /// `vsg_rs:` holds the options VSG does not have, kept out of its namespace.
    fn merge_extensions(&mut self, value: &Value) -> Result<(), String> {
        let map = value.as_mapping().ok_or("`vsg_rs` must be a mapping")?;
        for (key, value) in map {
            match key.as_str().unwrap_or_default() {
                "testbench_files" => {
                    let list = value
                        .as_sequence()
                        .ok_or("`testbench_files` must be a list of patterns")?;
                    self.testbench_files = list
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|p| p.replace('\\', "/").trim_start_matches("./").to_owned())
                        .collect();
                }
                "testbench_libraries" => {
                    let list = value
                        .as_sequence()
                        .ok_or("`testbench_libraries` must be a list of library names")?;
                    self.testbench_libraries = list
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|name| name.trim().to_ascii_lowercase())
                        .collect();
                }
                "synchronizers" => {
                    let list = value
                        .as_sequence()
                        .ok_or("`synchronizers` must be a list of entity names")?;
                    self.synchronizers = list
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|name| name.trim().to_ascii_lowercase())
                        .collect();
                }
                kind @ ("rtl" | "testbench") => {
                    let rule = value
                        .as_mapping()
                        .and_then(|m| m.get("rule"))
                        .ok_or_else(|| format!("`vsg_rs: {kind}` needs a `rule` block"))?;
                    // Validated now, applied per file once its kind is known.
                    let mut probe = self.clone();
                    probe.merge_rules(rule)?;
                    self.kind_rules.insert(kind.to_owned(), rule.clone());
                }
                "reflow_comments" => {
                    self.format.reflow_comments = value
                        .as_bool()
                        .ok_or("`reflow_comments` must be true or false")?;
                }
                key => self.warn(&format!("unknown `vsg_rs` key `{key}` ignored")),
            }
        }
        Ok(())
    }

    fn merge_pragma(&mut self, value: &Value) -> Result<(), String> {
        let patterns = value.as_mapping().and_then(|m| m.get("patterns"));
        let list = |key: &str| -> Result<Vec<String>, String> {
            let Some(items) = patterns
                .and_then(|p| p.as_mapping())
                .and_then(|p| p.get(key))
            else {
                return Ok(Vec::new());
            };
            let items = items
                .as_sequence()
                .ok_or_else(|| format!("`pragma.patterns.{key}` must be a list"))?;
            items
                .iter()
                .map(|i| {
                    let text = i.as_str().ok_or("pragma patterns must be strings")?;
                    regex::Regex::new(text).map_err(|e| format!("pragma pattern {text:?}: {e}"))?;
                    Ok(text.to_owned())
                })
                .collect()
        };
        let (open, close) = (list("open")?, list("close")?);
        if !open.is_empty() || !close.is_empty() {
            self.pragma_patterns = Some((open, close));
        }
        Ok(())
    }

    /// `file_list`: paths or glob patterns, each optionally mapped to its own `rule` block.
    fn merge_file_list(&mut self, value: &Value) -> Result<(), String> {
        let items = value
            .as_sequence()
            .ok_or("`file_list` must be a list of files")?;
        for item in items {
            match item {
                Value::String(pattern) => {
                    self.file_list
                        .push((pattern.replace('\\', "/"), PathBuf::new()));
                }
                Value::Mapping(map) => {
                    for (pattern, settings) in map {
                        let pattern = pattern
                            .as_str()
                            .ok_or("`file_list` entries must be paths")?
                            .replace('\\', "/");
                        if let Some(rule) = settings.as_mapping().and_then(|m| m.get("rule")) {
                            Config::default().merge_rules(rule)?;
                            self.file_rules.push((pattern.clone(), rule.clone()));
                        }
                        self.file_list.push((pattern, PathBuf::new()));
                    }
                }
                _ => return Err("`file_list` entries must be paths".into()),
            }
        }
        Ok(())
    }

    /// `file_rules` as a mapping `{pattern: {rule: ...}}` or a list of such mappings.
    fn merge_file_rules(&mut self, value: &Value) -> Result<(), String> {
        let entries: Vec<&Value> = match value {
            Value::Sequence(items) => items.iter().collect(),
            other => vec![other],
        };
        for entry in entries {
            let Some(map) = entry.as_mapping() else {
                return Err("`file_rules` entries must map a path to settings".into());
            };
            for (pattern, settings) in map {
                let pattern = pattern
                    .as_str()
                    .ok_or("`file_rules` keys must be paths")?
                    .replace('\\', "/");
                let rule = settings
                    .as_mapping()
                    .and_then(|m| m.get("rule"))
                    .ok_or_else(|| format!("`file_rules.{pattern}` must contain `rule`"))?;
                // Validate now so errors are reported when loading.
                Config::default().merge_rules(rule)?;
                self.file_rules.push((pattern, rule.clone()));
            }
        }
        Ok(())
    }

    /// The configuration for a particular file, with matching `file_rules` applied.
    pub fn for_path(&self, path: &Path) -> std::borrow::Cow<'_, Config> {
        let path = path.to_string_lossy().replace('\\', "/");
        let matching: Vec<&Value> = self
            .file_rules
            .iter()
            .filter(|(pattern, _)| path_matches(pattern, &path))
            .map(|(_, rule)| rule)
            .collect();
        if matching.is_empty() {
            return std::borrow::Cow::Borrowed(self);
        }
        let mut cfg = self.clone();
        for rule in matching {
            // Already validated when loading.
            let _ = cfg.merge_rules(rule);
        }
        cfg.resolve_format();
        std::borrow::Cow::Owned(cfg)
    }

    /// The configuration for a file of this kind (`rtl` or `testbench`), which is the ordinary
    /// configuration with that kind's `rule` block merged over it.
    pub fn for_kind(&self, kind: &str) -> std::borrow::Cow<'_, Config> {
        let Some(rule) = self.kind_rules.get(kind) else {
            return std::borrow::Cow::Borrowed(self);
        };
        let mut cfg = self.clone();
        // Already validated when loading.
        let _ = cfg.merge_rules(rule);
        cfg.resolve_format();
        std::borrow::Cow::Owned(cfg)
    }

    fn merge_rules(&mut self, value: &Value) -> Result<(), String> {
        let Some(map) = value.as_mapping() else {
            return Err("`rule` must be a mapping".into());
        };
        for (key, settings) in map {
            let key = key.as_str().unwrap_or_default();
            match key {
                "global" => merge_layer(&mut self.global, settings, key)?,
                "group" => {
                    let Some(groups) = settings.as_mapping() else {
                        return Err("`rule.group` must be a mapping".into());
                    };
                    for (name, settings) in groups {
                        let name = name.as_str().unwrap_or_default().to_owned();
                        let layer = self.groups.entry(name.clone()).or_default();
                        merge_layer(layer, settings, &name)?;
                    }
                }
                id => {
                    if !crate::rules::is_known_rule(id) {
                        self.unknown_rules.push(id.to_owned());
                    }
                    merge_layer(self.rules.entry(id.to_owned()).or_default(), settings, id)?;
                }
            }
        }
        Ok(())
    }

    fn warn(&mut self, message: &str) {
        self.warnings.push(message.to_owned());
    }

    /// Map VSG rule options that are formatter policy onto the formatter configuration.
    fn resolve_format(&mut self) {
        // Settings of local rules are for VSG.
        for id in std::mem::take(&mut self.unknown_rules) {
            if self.local_rules.is_none() {
                self.warn(&format!(
                    "rule `{id}` is not implemented by vsg-rs; its settings are ignored"
                ));
            }
        }
        if let Some(width) = self.layer_option("length_001", &["length"], "length")
            && let Some(width) = width.as_u64().and_then(|w| usize::try_from(w).ok())
        {
            self.format.width = width;
        }
        if let Some(size) = self
            .global
            .options
            .get("indent_size")
            .and_then(Value::as_u64)
        {
            self.format.indent = usize::try_from(size).unwrap_or(2);
        }
        match self
            .global
            .options
            .get("indent_style")
            .and_then(Value::as_str)
        {
            Some("smart_tabs") => self.format.tabs = true,
            Some("spaces") | None => {}
            Some(other) => self.warn(&format!("indent_style `{other}` is not supported")),
        }
        let case = ["case::keyword", "case"]
            .iter()
            .find_map(|g| self.groups.get(*g).and_then(|l| l.options.get("case")))
            .or_else(|| self.global.options.get("case"))
            .and_then(Value::as_str);
        match case {
            Some("lower") => self.format.keyword_case = KeywordCase::Lower,
            Some("upper") => self.format.keyword_case = KeywordCase::Upper,
            Some(other) => self.warn(&format!("keyword case `{other}` is not supported")),
            None => {}
        }
        self.resolve_blank_lines();
        self.resolve_alignment();
        self.resolve_spacing();
        let tokens = self
            .raw_indent
            .as_ref()
            .map(|i| i["tokens"].clone())
            .filter(|t| !t.is_null());
        let (policy, warnings) = crate::indent::resolve(tokens.as_ref());
        self.format.indent_policy = policy;
        for w in warnings {
            self.warn(&w);
        }
        let keyword_case_disabled = ["case::keyword", "case"]
            .iter()
            .any(|g| self.groups.get(*g).and_then(|l| l.disable) == Some(true));
        if keyword_case_disabled {
            self.format.keyword_case = KeywordCase::Preserve;
        }
        self.resolve_keyword_rules();
    }

    /// Per-rule keyword case: a rule that is disabled or configured with another case than the
    /// keyword group applies to its keywords inside its construct (everywhere for operator
    /// rules).
    fn resolve_keyword_rules(&mut self) {
        let default = self.format.keyword_case;
        let mut everywhere: BTreeMap<String, KeywordCase> = BTreeMap::new();
        let mut contextual: std::collections::HashMap<
            String,
            Vec<(vhdl_syntax::syntax::NodeKind, KeywordCase)>,
        > = std::collections::HashMap::new();
        let mut conflicts = Vec::new();
        for (info, words) in crate::keywords::RULES {
            let settings = self.rule(info);
            let case = if !settings.enabled || !settings.fixable {
                KeywordCase::Preserve
            } else {
                match settings.option_str("case") {
                    Some("upper") => KeywordCase::Upper,
                    Some("lower") => KeywordCase::Lower,
                    _ => default,
                }
            };
            if case == default {
                continue;
            }
            let kinds = crate::keywords::constructs(info.id);
            for word in *words {
                if kinds.is_empty() {
                    if everywhere
                        .insert((*word).to_owned(), case)
                        .is_some_and(|c| c != case)
                    {
                        conflicts.push(format!("`{word}`"));
                    }
                    continue;
                }
                let entries = contextual.entry((*word).to_owned()).or_default();
                for kind in kinds {
                    match entries.iter().find(|(k, _)| k == kind) {
                        Some((_, c)) if *c != case => {
                            conflicts.push(format!("`{word}` ({})", info.id));
                        }
                        Some(_) => {}
                        None => entries.push((*kind, case)),
                    }
                }
            }
        }
        conflicts.dedup();
        if !conflicts.is_empty() {
            let message = format!(
                "keyword case rules disagree for {}; the first rule wins",
                conflicts.join(", ")
            );
            self.warn(&message);
        }
        self.format.keyword_case_overrides = everywhere;
        self.format.keyword_case_in = contextual;
    }

    /// VSG options that set spaces, parenthesis placement and continuation style.
    fn resolve_spacing(&mut self) {
        use vhdl_syntax::syntax::NodeKind;
        let number = |s: &RuleSettings, key: &str| {
            s.options
                .get(key)
                .and_then(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|t| t.trim().parse().ok()))
                })
                .and_then(|n| usize::try_from(n).ok())
        };
        let mut spacing = Vec::new();
        let defaults = &crate::vsg_defaults::defaults()["rule"];
        for (rule, pairs) in crate::layout::spacing_pairs() {
            let Some(settings) = self.rule_by_id(rule).filter(|s| s.enabled) else {
                continue;
            };
            let Some(value) = settings.options.get("number_of_spaces") else {
                continue;
            };
            let Some(text) = value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.as_u64().map(|n| n.to_string()))
            else {
                self.warn(&format!(
                    "{rule}: number_of_spaces must be a number or \">=N\""
                ));
                continue;
            };
            let default = &defaults[rule.as_str()]["number_of_spaces"];
            let default = default
                .as_str()
                .map(str::to_owned)
                .or_else(|| default.as_u64().map(|n| n.to_string()));
            if default.as_deref() == Some(text.as_str()) {
                continue;
            }
            let text = text.trim();
            let spaces = match text.strip_prefix(">=").map(str::trim) {
                Some(n) => n.parse().ok().map(crate::layout::Spaces::AtLeast),
                None => text.parse().ok().map(crate::layout::Spaces::Exact),
            };
            let Some(spaces) = spaces else {
                self.warn(&format!("{rule}: unsupported number_of_spaces `{text}`"));
                continue;
            };
            if spaces == crate::layout::Spaces::Exact(1) {
                continue;
            }
            for [prev, next] in pairs {
                spacing.push(crate::layout::SpacingRule {
                    kinds: crate::keywords::constructs(rule),
                    prev: prev.as_deref(),
                    next: next.as_deref(),
                    spaces,
                });
            }
        }
        self.format.spacing = spacing;
        for (i, rule) in ["port_007", "port_008", "port_009"].iter().enumerate() {
            if let Some(s) = self.rule_by_id(rule).filter(|s| s.enabled) {
                let (before, after) = self.format.mode_spacing[i];
                self.format.mode_spacing[i] = (
                    number(&s, "spaces_before").unwrap_or(before).max(1),
                    number(&s, "spaces_after").unwrap_or(after).max(1),
                );
            }
        }
        self.format.close_paren_same_line = [
            ("generic_010", NodeKind::GenericClause),
            ("port_014", NodeKind::PortClause),
            ("generic_map_004", NodeKind::GenericMapAspect),
            ("port_map_004", NodeKind::PortMapAspect),
        ]
        .into_iter()
        .filter(|(rule, _)| {
            self.rule_by_id(rule)
                .is_some_and(|s| s.enabled && s.option_str("action") == Some("same_line"))
        })
        .map(|(_, kind)| kind)
        .collect();
        // `comment_004` counts the spaces before an inline comment; `>=N` keeps wider source
        // spacing, which the formatter approximates by taking N as the minimum.
        if let Some(settings) = self.rule_by_id("comment_004").filter(|s| s.enabled) {
            let text = settings
                .option_str("number_of_spaces")
                .map(str::to_owned)
                .or_else(|| {
                    settings
                        .option_usize("number_of_spaces")
                        .map(|n| n.to_string())
                })
                .unwrap_or_default();
            let text = text.trim();
            if let Ok(n) = text
                .strip_prefix(">=")
                .unwrap_or(text)
                .trim()
                .parse::<usize>()
            {
                self.format.comment_spaces = n.max(1);
            }
        }
        self.format.indent_continuations =
            ["concurrent_003", "sequential_004"].iter().any(|rule| {
                self.rule_by_id(rule).is_some_and(|s| {
                    s.enabled
                        && s.option_str("align_left") == Some("yes")
                        && s.option_str("align_paren") == Some("no")
                })
            });
    }

    fn resolve_alignment(&mut self) {
        let mut align = crate::align::AlignSettings::default();
        for (info, family) in crate::align::RULES.iter().zip(align.families_mut()) {
            let settings = self.rule(info);
            let yes = |key: &str, default: bool| match settings.option_str(key) {
                Some(v) => v == "yes",
                None => settings.option_bool(key).unwrap_or(default),
            };
            family.enabled = settings.enabled;
            family.blank_line_ends_group =
                yes("blank_line_ends_group", family.blank_line_ends_group);
            family.comment_line_ends_group =
                yes("comment_line_ends_group", family.comment_line_ends_group);
            family.include_lines_without_comments = yes(
                "include_lines_without_comments",
                family.include_lines_without_comments,
            );
            family.compact = yes("compact_alignment", family.compact);
        }
        let enabled = |rule: &str| self.rule_by_id(rule).is_none_or(|s| s.enabled);
        align.interface_colons = enabled("entity_017");
        align.component_colons = enabled("component_017");
        align.parameter_colons = enabled("procedure_410");
        align.map_arrows = enabled("instantiation_010");
        // `compact_alignment` of the same rules: `no` lets a group keep a wider column it
        // already agrees on, which is how a project pads its interface lists on purpose.
        let compact = |id: &str| {
            self.rule_by_id(id).is_none_or(|s| {
                s.option_str("compact_alignment").map_or_else(
                    || s.option_bool("compact_alignment").unwrap_or(true),
                    |v| v == "yes",
                )
            })
        };
        align.interface_assignments = enabled("entity_018");
        align.interface_assignments_compact = compact("entity_018");
        align.interface_colons_compact = compact("entity_017");
        align.component_colons_compact = compact("component_017");
        align.parameter_colons_compact = compact("procedure_410");
        align.map_arrows_compact = compact("instantiation_010");
        self.format.align = align;
    }

    fn resolve_blank_lines(&mut self) {
        use crate::blank::{BlankSettings, Pragmas, Style};
        let mut blank = BlankSettings::preserve();
        for rule in crate::blank::rules() {
            let settings = self.rule(&rule.info);
            if !settings.enabled {
                continue;
            }
            let style = match settings.option_str("style") {
                Some(text) => {
                    if let Some(style) = Style::parse(text) {
                        Some(style)
                    } else {
                        self.warn(&format!("{}: unsupported style `{text}`", rule.info.id));
                        rule.style
                    }
                }
                None => rule.style,
            };
            if rule.info.id == "library_003" {
                blank.allow_library_clause =
                    settings.option_str("allow_library_clause") == Some("yes");
            }
            blank.rules.insert(rule.info.id, style);
        }
        let limit = self.rule(&WHITESPACE_200);
        blank.max_blank_lines = if limit.enabled {
            limit.option_usize("blank_lines_allowed").unwrap_or(1)
        } else {
            usize::from(u8::MAX)
        };
        let pragmas_enabled = PRAGMA_RULES.iter().any(|r| self.rule(r).enabled);
        blank.pragmas = pragmas_enabled.then(|| match &self.pragma_patterns {
            Some((open, close)) => {
                let open: Vec<&str> = open.iter().map(String::as_str).collect();
                let close: Vec<&str> = close.iter().map(String::as_str).collect();
                Pragmas::new(&open, &close)
            }
            None => Pragmas::default(),
        });
        self.format.blank = blank;
    }

    fn layer_option(&self, rule: &str, groups: &[&str], key: &str) -> Option<&Value> {
        self.rules
            .get(rule)
            .and_then(|l| l.options.get(key))
            .or_else(|| {
                groups
                    .iter()
                    .find_map(|g| self.groups.get(*g)?.options.get(key))
            })
            .or_else(|| self.global.options.get(key))
    }

    /// Effective settings for a rule: rule-specific settings override its groups, which
    /// override `global`, which override the rule's defaults.
    pub fn rule(&self, info: &crate::rules::RuleInfo) -> RuleSettings {
        self.layered(
            info.id,
            info.groups,
            info.enabled_by_default,
            info.severity,
            BTreeMap::new(),
        )
    }

    /// Settings of any VSG rule by id, starting from VSG's defaults.
    pub fn rule_by_id(&self, id: &str) -> Option<RuleSettings> {
        // The lint layer's rules are not VSG's, so they have no entry in its defaults: they are
        // enabled and error by default, and the configuration layers over that as usual.
        if id.starts_with("lint_") {
            // Most lint rules are on once the layer runs. Two kinds start off: house style,
            // which is a project's choice, and the experimental rules, which infer design intent
            // rather than deriving it and so cannot point at the evidence the others can.
            let on_by_default = !matches!(id, "lint_602" | "lint_603" | "lint_700" | "lint_713");
            return Some(self.layered(
                id,
                &["lint"],
                on_by_default,
                Severity::Error,
                BTreeMap::new(),
            ));
        }
        let defaults = crate::vsg_defaults::defaults()["rule"].get(id)?;
        let mut options: BTreeMap<String, Value> = BTreeMap::new();
        for (k, v) in defaults.as_object().into_iter().flatten() {
            if !matches!(k.as_str(), "disable" | "severity" | "fixable")
                && let Ok(v) = yaml_serde::to_value(v)
            {
                options.insert(k.clone(), v);
            }
        }
        let mut groups: Vec<&'static str> = crate::vsg_defaults::groups(id).to_vec();
        if let Some(info) = crate::rules::info(id) {
            for g in info.groups {
                if !groups.contains(g) {
                    groups.push(g);
                }
            }
        }
        let severity = if defaults["severity"] == "Warning" {
            Severity::Warning
        } else {
            Severity::Error
        };
        let enabled = !defaults["disable"].as_bool().unwrap_or(false);
        let settings = self.layered(id, &groups, enabled, severity, options);
        Some(settings)
    }

    /// Disable every rule outside `group` (VSG `--style indent_only`).
    pub fn enable_only_group(&mut self, group: &str) {
        self.global.disable = Some(true);
        self.groups.entry(group.to_owned()).or_default().disable = Some(false);
    }

    /// Number of enabled VSG rules.
    pub fn enabled_rule_count(&self) -> usize {
        crate::vsg_defaults::rule_ids()
            // VSG does not count rules without a phase.
            .filter(|id| {
                crate::vsg_defaults::defaults()["rule"][*id]
                    .get("phase")
                    .is_some()
            })
            .filter(|id| self.rule_by_id(id).is_some_and(|s| s.enabled))
            .count()
    }

    /// The configuration of one rule as `vsg -rc` prints it.
    pub fn rule_configuration(&self, id: &str) -> Option<serde_json::Value> {
        // VSG prints the common attributes first.
        const COMMON: [&str; 7] = [
            "indent_style",
            "indent_size",
            "phase",
            "disable",
            "fixable",
            "severity",
            "user_error_message",
        ];
        let s = self.rule_by_id(id)?;
        let mut out = serde_json::Map::new();
        let defaults = &crate::vsg_defaults::defaults()["rule"][id];
        let mut keys: Vec<(&String, &serde_json::Value)> =
            defaults.as_object().into_iter().flatten().collect();
        keys.sort_by_key(|(k, _)| COMMON.iter().position(|c| c == k).unwrap_or(COMMON.len()));
        for (k, v) in keys {
            let value = match k.as_str() {
                "disable" => serde_json::Value::Bool(!s.enabled),
                "fixable" => serde_json::Value::Bool(s.fixable),
                "severity" => serde_json::Value::String(match s.severity {
                    Severity::Error => "Error".into(),
                    Severity::Warning => "Warning".into(),
                }),
                _ => s
                    .options
                    .get(k)
                    .and_then(|v| serde_json::to_value(v).ok())
                    .unwrap_or_else(|| v.clone()),
            };
            out.insert(k.clone(), value);
        }
        Some(serde_json::Value::Object(out))
    }

    /// The whole effective configuration as `vsg -oc` writes it.
    pub fn effective_configuration(&self) -> serde_json::Value {
        let defaults = crate::vsg_defaults::defaults();
        let overlay = |base: &serde_json::Value, raw: &Option<serde_json::Value>| {
            let mut base = base.clone();
            if let Some(raw) = raw {
                merge_json(&mut base, raw);
            }
            base
        };
        let rules: serde_json::Map<String, serde_json::Value> = crate::vsg_defaults::rule_ids()
            .filter_map(|id| Some((id.to_owned(), self.rule_configuration(id)?)))
            .collect();
        let mut out = serde_json::json!({
            "indent": overlay(&defaults["indent"], &self.raw_indent),
            "pragma": overlay(&defaults["pragma"], &self.raw_pragma),
            "rule": rules,
        });
        if let Some(dir) = &self.local_rules {
            out["local_rules"] = dir.to_string_lossy().into();
        }
        out
    }

    fn layered(
        &self,
        id: &str,
        groups: &[&str],
        enabled: bool,
        severity: Severity,
        options: BTreeMap<String, Value>,
    ) -> RuleSettings {
        let mut layers = vec![&self.global];
        layers.extend(groups.iter().filter_map(|g| self.groups.get(*g)));
        layers.extend(self.rules.get(id));
        let mut settings = RuleSettings {
            enabled,
            severity,
            fixable: crate::vsg_defaults::defaults()["rule"][id]["fixable"]
                .as_bool()
                .unwrap_or(true),
            fixable_configured: false,
            options,
        };
        for layer in layers {
            if let Some(disable) = layer.disable {
                settings.enabled = !disable;
            }
            if let Some(severity) = layer.severity {
                settings.severity = severity;
            }
            if let Some(fixable) = layer.fixable {
                settings.fixable = fixable;
                settings.fixable_configured = true;
            }
            for (k, v) in &layer.options {
                settings.options.insert(k.clone(), v.clone());
            }
        }
        settings
    }
}

/// Merge a configuration block into the JSON copy kept for `-oc` (mappings recursively).
fn merge_raw(base: &mut Option<serde_json::Value>, overlay: &Value) {
    let Ok(overlay) = serde_json::to_value(overlay) else {
        return;
    };
    match base {
        Some(base) => merge_json(base, &overlay),
        None => *base = Some(overlay),
    }
}

fn merge_json(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(existing) => merge_json(existing, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// Whether `path` matches `pattern` (`*` and `?` within a path component, `**` across
/// components). Relative patterns also match any path that ends with them.
fn path_matches(pattern: &str, path: &str) -> bool {
    if glob(pattern.as_bytes(), path.as_bytes()) {
        return true;
    }
    !pattern.starts_with('/')
        && path
            .match_indices('/')
            .any(|(i, _)| glob(pattern.as_bytes(), &path.as_bytes()[i + 1..]))
}

/// Match a path against a VSG-style pattern: `?` one character, `*` one path segment, `**` any
/// number of segments. Both sides use `/` separators.
pub fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern {
        [] => text.is_empty(),
        [b'*', b'*', rest @ ..] => {
            let rest = rest.strip_prefix(b"/").unwrap_or(rest);
            (0..=text.len()).any(|i| glob(rest, &text[i..]))
        }
        [b'*', rest @ ..] => (0..=text.len())
            .take_while(|&i| i == 0 || text[i - 1] != b'/')
            .any(|i| glob(rest, &text[i..])),
        [b'?', rest @ ..] => text.first().is_some_and(|&c| c != b'/') && glob(rest, &text[1..]),
        [c, rest @ ..] => text.first() == Some(c) && glob(rest, &text[1..]),
    }
}

fn merge_layer(layer: &mut RuleLayer, settings: &Value, name: &str) -> Result<(), String> {
    let Some(map) = settings.as_mapping() else {
        return Err(format!("settings of `{name}` must be a mapping"));
    };
    for (key, value) in map {
        let key = key.as_str().unwrap_or_default();
        let bad = || format!("`{name}.{key}` has an invalid value {value:?}");
        match key {
            "disable" => layer.disable = Some(value.as_bool().ok_or_else(bad)?),
            "fixable" => layer.fixable = Some(value.as_bool().ok_or_else(bad)?),
            "severity" => {
                layer.severity = Some(
                    match value.as_str().map(str::to_ascii_lowercase).as_deref() {
                        Some("error") => Severity::Error,
                        Some("warning") => Severity::Warning,
                        _ => return Err(bad()),
                    },
                );
            }
            // VSG phases do not exist in vsg-rs; see docs/compatibility.md.
            "phase" | "user_error_message" => {}
            _ => {
                layer.options.insert(key.to_owned(), value.clone());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_variables_in_file_list() {
        let home = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        assert_eq!(
            expand_vars("${CARGO_MANIFEST_DIR}/src/$CARGO_MANIFEST_DIR/$VSG_RS_UNSET_VARIABLE"),
            format!("{home}/src/{home}/$VSG_RS_UNSET_VARIABLE")
        );
    }

    #[test]
    fn vsg_style_configuration() {
        let cfg = Config::parse(
            "rule:\n  global:\n    indent_size: 4\n  group:\n    case::keyword:\n      case: upper\n  length_001:\n    length: 100\n  entity_015:\n    disable: true\n    severity: warning\nlinesep: \"\\r\\n\"\nindent:\n  tokens: {}\n",
        )
        .unwrap();
        assert_eq!(cfg.format.width, 100);
        assert_eq!(cfg.format.indent, 4);
        assert_eq!(cfg.format.keyword_case, KeywordCase::Upper);
        let per_rule = Config::parse(
            "rule:\n  entity_004:\n    case: upper\n  entity_010:\n    disable: true\n",
        )
        .unwrap();
        let out = crate::format(b"Entity e is\nEnd Entity;\n".to_vec(), &per_rule.format).unwrap();
        assert_eq!(out, b"ENTITY e is\nEnd ENTITY;\n");
        assert_eq!(cfg.format.line_ending, Some(LineEnding::CrLf));
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        assert!(!cfg.format.tabs);
        let tabs = Config::parse("rule:\n  global:\n    indent_style: smart_tabs\n").unwrap();
        assert!(tabs.format.tabs);
        let out = crate::format(
            b"entity e is\nport (a : bit);\nend;\n".to_vec(),
            &tabs.format,
        )
        .unwrap();
        assert_eq!(out, b"entity e is\n\tport (\n\t\ta : bit\n\t);\nend;\n");
        let info = crate::rules::info("entity_015").unwrap();
        let rule = cfg.rule(info);
        assert!(!rule.enabled);
        assert_eq!(rule.severity, Severity::Warning);
    }

    #[test]
    fn precedence_global_group_rule() {
        let cfg = Config::parse(
            "rule:\n  global:\n    disable: true\n  group:\n    structure::optional:\n      disable: false\n  entity_019:\n    disable: true\n",
        )
        .unwrap();
        assert!(cfg.rule(crate::rules::info("entity_015").unwrap()).enabled);
        assert!(!cfg.rule(crate::rules::info("entity_019").unwrap()).enabled);
        assert!(!cfg.rule(crate::rules::info("process_016").unwrap()).enabled);
    }

    #[test]
    fn file_rules() {
        let cfg = Config::parse(
            "rule: {length_001: {length: 100}}\nfile_rules:\n  legacy/**/*.vhd:\n    rule: {length_001: {length: 200}, entity_015: {disable: true}}\n",
        )
        .unwrap();
        let info = crate::rules::info("entity_015").unwrap();
        let legacy = cfg.for_path(Path::new("/src/legacy/old/core.vhd"));
        assert_eq!(legacy.format.width, 200);
        assert!(!legacy.rule(info).enabled);
        let other = cfg.for_path(Path::new("/src/new/core.vhd"));
        assert_eq!(other.format.width, 100);
        assert!(other.rule(info).enabled);
        assert!(path_matches("a/*.vhd", "x/a/b.vhd"));
        assert!(!path_matches("a/*.vhd", "x/a/b/c.vhd"));
        assert!(path_matches("a/**/c.vhd", "a/c.vhd"));
        assert!(Config::parse("file_rules: {x.vhd: {}}").is_err());
    }

    #[test]
    fn json_and_errors() {
        let cfg = Config::parse(r#"{"rule": {"length_001": {"length": 80}}}"#).unwrap();
        assert_eq!(cfg.format.width, 80);
        assert!(Config::parse("rule: {entity_015: {disable: maybe}}").is_err());
        assert!(
            Config::parse("rule: {no_such_rule_999: {disable: true}}")
                .unwrap()
                .warnings
                .iter()
                .any(|w| w.contains("no_such_rule_999"))
        );
    }
}

//! `vsg-rs lsp`: a deliberately narrow language server.
//!
//! It offers what vsg-rs is: diagnostics, and formatting. It does **not** offer completion,
//! hover, definition, references, rename, symbols or semantic tokens, and it does not advertise
//! them — those belong to a VHDL language server such as `vhdl_ls`, which this is meant to run
//! beside rather than replace.
//!
//! Everything it answers with comes from the same library the command line uses: the same parser,
//! the same formatter, the same analysis, the same configuration. There is no editor-specific
//! implementation of anything, so a diagnostic in an editor is a diagnostic on the command line.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp_server::jsonrpc::{self, Result};
use tower_lsp_server::ls_types::{
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, Diagnostic, DiagnosticRelatedInformation,
    DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, InitializeParams, InitializeResult,
    InitializedParams, Location, MessageType, NumberOrString, OneOf, Position, Range,
    ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit,
    Uri, WorkspaceEdit,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server};
use vsg_rs::Config;
use vsg_rs::analysis;

/// One open document, as the editor currently has it.
struct Document {
    text: String,
    version: i32,
}

/// One project's analyser, and the library map it was built from.
struct Analysed {
    analyser: analysis::lint::Analyser,
    /// What the map said when the project was built, so a map that has been edited since is
    /// noticed rather than believed.
    map: Option<(PathBuf, std::time::SystemTime)>,
}

pub(crate) struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<Uri, Document>>>,
    /// The analysed projects, kept between edits and keyed by the library map they belong to.
    /// Building one parses every file the map names plus the embedded `ieee` and `std`, which is
    /// most of what an analysis costs and is the same work every time; only the edited buffer
    /// changes.
    ///
    /// Keyed, not single: one server is asked about files from every project the editor has open,
    /// and answering for one project out of another's symbol table is worse than answering
    /// slowly. The key is the library map that governs the file, or `None` for a file with no
    /// map at all.
    analysers: Arc<tokio::sync::Mutex<HashMap<Option<PathBuf>, Analysed>>>,
}

/// The configuration that applies to a file, found the way the command line finds it: the
/// nearest `vsg-rs.yaml` (or `.json`) in its directory or an ancestor.
fn config_for(path: &Path) -> std::result::Result<Config, String> {
    let Some(file) = path.parent().and_then(vsg_rs::config::discover) else {
        return Ok(Config::default().for_path(path).into_owned());
    };
    // Not `.ok()`: a configuration that does not load is not the default configuration. Treating
    // it as one lets format-on-save rewrite a file under settings the project never chose, while
    // the command line refuses to run at all -- the editor and CI would disagree about the same
    // file, which is the one thing this tool must not do.
    let cfg = Config::load(std::slice::from_ref(&file))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(cfg.for_path(path).into_owned())
}

/// The path an editor URI stands for. A document that is not a file still needs a name, because
/// the name decides the configuration and the library it is analysed in.
fn path_of(uri: &Uri) -> PathBuf {
    let path = uri.path().as_str();
    // A Windows file URI is `file:///C:/dir/x.vhd`, whose path component is `/C:/dir/x.vhd`.
    // Handed to the filesystem unchanged that is not a path at all, so the document would never
    // be found and no configuration would be discovered for it.
    let path = path
        .strip_prefix('/')
        .filter(|rest| {
            let mut chars = rest.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.next() == Some(':')
                && matches!(chars.next(), Some('/') | None)
        })
        .unwrap_or(path);
    // Percent-encoding is how a space or a `#` survives a URI; the filesystem wants it back.
    percent_decode(path).into()
}

/// `%20` and friends, decoded. Anything that is not a valid escape is left as it was.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let decoded = (bytes[at] == b'%')
            .then(|| text.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = decoded {
            out.push(byte);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A byte offset in `text`, as an LSP position. LSP counts UTF-16 code units within a line.
fn position_of(text: &str, offset: usize) -> Position {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let start = before.rfind('\n').map_or(0, |at| at + 1);
    Position {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        character: u32::try_from(text[start..offset].encode_utf16().count()).unwrap_or(u32::MAX),
    }
}

/// A one-based line and column, as an LSP position, for findings that carry no byte offset.
fn position_at(line: usize, column: usize) -> Position {
    Position {
        line: u32::try_from(line.saturating_sub(1)).unwrap_or(0),
        character: u32::try_from(column.saturating_sub(1)).unwrap_or(0),
    }
}

impl Backend {
    /// Analyse one document exactly as `--check style,lint` would, and publish the result.
    async fn publish(&self, uri: Uri, text: String, version: i32) {
        let path = path_of(&uri);
        // One analysis at a time: the project is shared, and two edits analysing it at once
        // would each see the other's buffer half applied.
        let analysers = Arc::clone(&self.analysers);
        let diagnostics = tokio::task::spawn_blocking({
            let path = path.clone();
            let text = text.clone();
            move || {
                let mut held = analysers.blocking_lock();
                diagnose(&path, &text, &mut held)
            }
        })
        .await
        .unwrap_or_default();
        // Analyses run concurrently and do not finish in the order they started: a small edit can
        // overtake the larger buffer before it. Publishing that late result would leave the editor
        // showing diagnostics for a version the user has already moved past -- or, if the document
        // has since been closed, diagnostics for a file that is no longer open, which `did_close`
        // has already withdrawn. Only the version that is still open is worth publishing.
        if self
            .documents
            .read()
            .await
            .get(&uri)
            .map(|document| document.version)
            != Some(version)
        {
            return;
        }
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }
}

/// Everything vsg-rs reports about one buffer: the style rules and the lint layer, from the same
/// entry points the command line calls.
fn diagnose(
    path: &Path,
    text: &str,
    analysers: &mut HashMap<Option<PathBuf>, Analysed>,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    // A configuration that does not load stops everything: every rule below, and the severity of
    // every finding, comes from it. Saying so once, where the editor shows problems, is the whole
    // report -- findings computed under settings the project did not choose would be fiction.
    let cfg = match config_for(path) {
        Ok(cfg) => cfg,
        Err(message) => {
            out.push(Diagnostic {
                range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("vsg-rs".to_owned()),
                message: format!(
                    "vsg-rs is not running on this file: its configuration could not be read. \
                     {message}"
                ),
                ..Diagnostic::default()
            });
            return out;
        }
    };
    let parsed = vsg_rs::Parsed::new(text.as_bytes().to_vec());

    // A file that does not parse gets its syntax errors and nothing else: every rule below would
    // be reasoning about a tree that does not represent the source.
    if !parsed.syntax_errors().is_empty() && !parsed.is_blank() {
        for error in parsed.syntax_errors() {
            out.push(Diagnostic {
                range: Range::new(
                    position_of(text, error.offset),
                    position_of(text, error.offset),
                ),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("vsg-rs".to_owned()),
                message: error.message.clone(),
                ..Diagnostic::default()
            });
        }
        return out;
    }

    for violation in vsg_rs::rules::check_with(&parsed, &cfg, None) {
        out.push(Diagnostic {
            range: Range::new(
                position_of(text, violation.start),
                position_of(text, violation.end),
            ),
            severity: Some(if violation.severity.to_string() == "warning" {
                DiagnosticSeverity::WARNING
            } else {
                DiagnosticSeverity::ERROR
            }),
            code: Some(NumberOrString::String(violation.rule.to_owned())),
            source: Some("vsg-rs".to_owned()),
            message: violation.message.clone(),
            ..Diagnostic::default()
        });
    }

    // The front end's rules too, so an editor sees what `--check style,lint` sees. They need the
    // project's library map; without one only a few of them report, exactly as on the command
    // line. The buffer stands in for the file, so an unsaved edit is what gets analysed.
    let sources = [analysis::lint::Source::buffer(
        path.to_path_buf(),
        text.as_bytes().to_vec(),
    )];
    // Which project this file belongs to, and what its map said a moment ago.
    let map = path
        .parent()
        .and_then(analysis::lint::project_config_for)
        .map(|at| {
            let stamp = std::fs::metadata(&at)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            (at, stamp)
        });
    let key = map.as_ref().map(|(at, _)| at.clone());
    // A map that has been edited since the project was built describes a different project.
    if analysers
        .get(&key)
        .is_some_and(|held| held.map.as_ref().map(|(_, was)| *was) != map.as_ref().map(|(_, m)| *m))
    {
        analysers.remove(&key);
    }

    let mut project_error = None;
    if !analysers.contains_key(&key) {
        match analysis::lint::Analyser::for_project(&sources, key.as_deref()) {
            Ok(analyser) => {
                analysers.insert(
                    key.clone(),
                    Analysed {
                        analyser,
                        map: map.clone(),
                    },
                );
            }
            // Not silence: an unreadable `vhdl_ls.toml` switches off most of the lint layer, and
            // a report that is quietly missing 53 rules looks exactly like a clean one.
            Err(message) => project_error = Some(message),
        }
    }
    let resolved = analysers
        .get_mut(&key)
        .map(|held| held.analyser.analyse(&sources));
    let front_end = match resolved {
        Some(analysis) => analysis.reportable(),
        // The project could not be built at all; the rules that need it simply do not report.
        None => Vec::new(),
    };
    if let Some(message) = project_error {
        out.push(Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("vsg-rs".to_owned()),
            message: format!(
                "The rules that resolve names across files are not running: the project's \
                 library map could not be read. {message}"
            ),
            ..Diagnostic::default()
        });
    }

    // The two halves count columns differently, and only one of them is what an editor wants.
    // A rule working from the syntax tree reports a display column: tabs expanded to tab stops,
    // one per character. LSP wants UTF-16 code units, so a finding after a tab would be pointed
    // at several characters too far right, and one after a non-BMP character one too few. The
    // front end already answers in UTF-16 (`vhdl_lang`'s `Position::character` is defined that
    // way), so its findings are taken as they are.
    let native: Vec<(analysis::lint::Finding, bool)> = analysis::findings_for(&parsed, path, &cfg)
        .into_iter()
        .map(|f| (f, true))
        .chain(front_end.into_iter().map(|f| (f, false)))
        .collect();
    let place = |line: usize, column: usize, from_the_tree: bool| {
        if from_the_tree {
            position_of(text, parsed.offset_of(line, column))
        } else {
            position_at(line, column)
        }
    };

    for (finding, from_the_tree) in native {
        if cfg.rule_by_id(finding.rule).is_some_and(|s| !s.enabled) {
            continue;
        }
        let at = place(finding.line, finding.column, from_the_tree);
        // Structured, not flattened into the message: an editor can jump to each one.
        let related: Vec<DiagnosticRelatedInformation> = finding
            .related
            .iter()
            .filter_map(|other| {
                // Not `format!("file://{path}")`: a space, a `#` or a Windows drive letter makes
                // that not a URI, and the `?` below would drop the location rather than say so.
                let uri = Uri::from_file_path(&other.file)?;
                // Only this buffer's own columns can be converted: another file's text is not
                // here to measure. Every native rule that carries related locations puts them
                // in the same file, so that is the case worth getting right.
                let here = from_the_tree && other.file == path;
                let at = place(other.line, other.column, here);
                Some(DiagnosticRelatedInformation {
                    location: Location {
                        uri,
                        range: Range::new(at, at),
                    },
                    message: other.message.clone(),
                })
            })
            .collect();
        out.push(Diagnostic {
            range: Range::new(at, at),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(finding.rule.to_owned())),
            source: Some("vsg-rs".to_owned()),
            message: finding.message.clone(),
            related_information: (!related.is_empty()).then_some(related),
            ..Diagnostic::default()
        });
    }
    out
}

/// Whether two ranges touch. An editor asks for the actions of a selection, which is usually a
/// cursor: a zero-width range on the line of the diagnostic.
fn overlaps(a: &Range, b: &Range) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// The edits of a safe fix, as the document sees them.
///
/// `--fix` applies edits in (start, rank, end) order and two of them can insert at one offset, so
/// the order is settled here and same-offset insertions are merged. An editor applies a
/// `WorkspaceEdit`'s edits as a set, and would otherwise be free to reverse them.
fn edits_of(text: &str, fix: &vsg_rs::rules::Fix) -> Vec<TextEdit> {
    let mut edits: Vec<&vsg_rs::rules::Edit> = fix.edits.iter().collect();
    edits.sort_by_key(|e| (e.start, e.rank, e.end));
    let mut merged: Vec<vsg_rs::rules::Edit> = Vec::new();
    for edit in edits {
        match merged.last_mut() {
            Some(last) if last.start == edit.start && last.end == edit.end => {
                last.text.push_str(&edit.text);
            }
            _ => merged.push(edit.clone()),
        }
    }
    merged
        .iter()
        .map(|edit| TextEdit {
            range: Range::new(position_of(text, edit.start), position_of(text, edit.end)),
            new_text: edit.text.clone(),
        })
        .collect()
}

/// One document's edits, as a workspace edit.
fn workspace_edit(uri: &Uri, edits: Vec<TextEdit>) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: Some(std::iter::once((uri.clone(), edits)).collect()),
        ..WorkspaceEdit::default()
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        // A server can be initialized again; nothing from a previous session should survive it.
        self.documents.write().await.clear();
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "vsg-rs".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            offset_encoding: None,
            capabilities: ServerCapabilities {
                // Whole documents: correctness first. An incremental sync is an optimisation
                // this has no measurement to justify.
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::SOURCE_FIX_ALL,
                        ]),
                        ..CodeActionOptions::default()
                    },
                )),
                // Everything else is deliberately absent. vsg-rs is not a VHDL language server:
                // completion, hover, definition, references, rename and symbols belong to one,
                // and advertising them would make editors ask vsg-rs instead of asking it.
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "vsg-rs: diagnostics and formatting")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        // Documents live only in memory; dropping them is all there is to wind down.
        self.documents.write().await.clear();
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        self.documents.write().await.insert(
            document.uri.clone(),
            Document {
                text: document.text.clone(),
                version: document.version,
            },
        );
        self.publish(document.uri, document.text, document.version)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.into_iter().next_back() else {
            return;
        };
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        {
            let mut documents = self.documents.write().await;
            let document = documents.entry(uri.clone()).or_insert_with(|| Document {
                text: String::new(),
                version,
            });
            // An edit that arrives out of order is not what the editor has; publishing from it
            // would leave diagnostics describing a buffer that no longer exists.
            if version < document.version {
                return;
            }
            document.text.clone_from(&change.text);
            document.version = version;
        }
        self.publish(uri, change.text, version).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.write().await.remove(&uri);
        // The editor stops showing a closed file's diagnostics only if they are withdrawn.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let Some(text) = self
            .documents
            .read()
            .await
            .get(&uri)
            .map(|d| d.text.clone())
        else {
            return Ok(None);
        };
        let range = params.range;
        let wanted = params.context.only.clone();
        let path = path_of(&uri);
        let actions = tokio::task::spawn_blocking({
            let uri = uri.clone();
            move || {
                // An editor asking for one kind of action should not be given the others.
                let allows = |kind: &CodeActionKind| {
                    wanted
                        .as_ref()
                        .is_none_or(|only| only.iter().any(|k| k == kind))
                };
                // No configuration, no actions: a fix computed under the default settings is
                // not the fix `--fix` would apply, and applying it would be a silent edit.
                let Ok(cfg) = config_for(&path) else {
                    return Vec::new();
                };
                let parsed = vsg_rs::Parsed::new(text.as_bytes().to_vec());
                let mut actions: Vec<CodeActionOrCommand> = Vec::new();

                // One action per violation the cursor is on, from the fix it already carries.
                // Only a fix vsg-rs would apply itself is offered: the same test `--fix` uses, so
                // an editor never offers something the command line would refuse.
                if allows(&CodeActionKind::QUICKFIX) {
                    for violation in vsg_rs::rules::check_with(&parsed, &cfg, None) {
                        let Some(fix) = violation
                            .fix
                            .as_ref()
                            .filter(|f| f.safety == vsg_rs::rules::FixSafety::Safe)
                        else {
                            continue;
                        };
                        let at = Range::new(
                            position_of(&text, violation.start),
                            position_of(&text, violation.end),
                        );
                        if !overlaps(&at, &range) {
                            continue;
                        }
                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: format!("{}: {}", violation.rule, violation.message),
                            kind: Some(CodeActionKind::QUICKFIX),
                            edit: Some(workspace_edit(&uri, edits_of(&text, fix))),
                            ..CodeAction::default()
                        }));
                    }
                }

                // Fix all: the whole document as `--fix` would write it, which applies the safe
                // fixes and formats. Unsafe fixes are not part of it, here or there.
                if allows(&CodeActionKind::SOURCE_FIX_ALL) {
                    let fixed = vsg_rs::fix_with(&parsed, &cfg, &vsg_rs::FixOptions::default())
                        .ok()
                        .map(|out| out.output)
                        .and_then(|out| String::from_utf8(out).ok());
                    if let Some(fixed) = fixed.filter(|fixed| *fixed != text) {
                        let whole = Range::new(Position::new(0, 0), position_of(&text, text.len()));
                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: "Fix all vsg-rs findings".to_owned(),
                            kind: Some(CodeActionKind::SOURCE_FIX_ALL),
                            edit: Some(workspace_edit(
                                &uri,
                                vec![TextEdit {
                                    range: whole,
                                    new_text: fixed,
                                }],
                            )),
                            ..CodeAction::default()
                        }));
                    }
                }
                actions
            }
        })
        .await
        .unwrap_or_default();
        Ok(Some(actions))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let Some(text) = self
            .documents
            .read()
            .await
            .get(&uri)
            .map(|d| d.text.clone())
        else {
            return Ok(None);
        };
        let path = path_of(&uri);
        // The same entry point `--fix` uses, so formatting on save leaves a file that the
        // command line then reports nothing about. Formatting alone would leave the safe rule
        // fixes unapplied and CI would disagree with the editor.
        let formatted = tokio::task::spawn_blocking({
            let source = text.as_bytes().to_vec();
            move || {
                let cfg = config_for(&path)?;
                let parsed = vsg_rs::Parsed::new(source);
                Ok(
                    vsg_rs::fix_with(&parsed, &cfg, &vsg_rs::FixOptions::default())
                        .ok()
                        .map(|out| out.output),
                )
            }
        })
        .await
        .map_err(|_| jsonrpc::Error::internal_error())?;
        // Refusing is the point: formatting under the default settings would rewrite the file
        // the project's own configuration does not describe, and the editor would disagree with
        // what the command line does to the same file.
        let formatted = formatted.map_err(|message: String| jsonrpc::Error {
            code: jsonrpc::ErrorCode::InvalidParams,
            message: format!("vsg-rs did not format this file: {message}").into(),
            data: None,
        })?;
        // A file that does not parse is left alone, as `--fix` leaves it alone.
        let Some(formatted) = formatted else {
            return Ok(None);
        };
        let Ok(formatted) = String::from_utf8(formatted) else {
            return Ok(None);
        };
        if formatted == text {
            return Ok(Some(Vec::new()));
        }
        // One edit for the whole document: the formatter decides a canonical layout for the file,
        // not a set of local changes, and a minimal diff would be an invention on top of it.
        Ok(Some(vec![TextEdit {
            range: Range::new(Position::new(0, 0), position_of(&text, text.len())),
            new_text: formatted,
        }]))
    }
}

/// Serve on stdin and stdout until the client disconnects.
pub(crate) fn serve() -> std::process::ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("ERROR: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    runtime.block_on(async {
        let (service, socket) = LspService::new(|client| Backend {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
            analysers: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        });
        Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
            .serve(service)
            .await;
    });
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(text: &str) -> Uri {
        text.parse().expect("a URI")
    }

    #[test]
    fn a_windows_uri_becomes_a_windows_path() {
        // The path component of `file:///C:/dir/x.vhd` starts with a slash the filesystem has
        // no use for; leaving it on means no document is ever found on Windows.
        assert_eq!(
            path_of(&uri("file:///C:/dir/x.vhd")),
            PathBuf::from("C:/dir/x.vhd")
        );
        // A leading slash that is not a drive letter is part of the path.
        assert_eq!(
            path_of(&uri("file:///home/me/x.vhd")),
            PathBuf::from("/home/me/x.vhd")
        );
    }

    #[test]
    fn an_escaped_path_is_decoded() {
        assert_eq!(
            path_of(&uri("file:///home/my%20designs/x.vhd")),
            PathBuf::from("/home/my designs/x.vhd")
        );
        // Something that is not an escape is left alone rather than eaten.
        assert_eq!(percent_decode("100%"), "100%");
    }
}

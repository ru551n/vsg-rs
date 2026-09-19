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
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentFormattingParams,
    InitializeParams, InitializeResult, InitializedParams, Location, MessageType, NumberOrString,
    OneOf, Position, Range, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Uri,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server};
use vsg_rs::Config;
use vsg_rs::analysis;

/// One open document, as the editor currently has it.
struct Document {
    text: String,
    version: i32,
}

pub(crate) struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<Uri, Document>>>,
}

/// The configuration that applies to a file, found the way the command line finds it: the
/// nearest `vsg-rs.yaml` (or `.json`) in its directory or an ancestor.
fn config_for(path: &Path) -> Config {
    let found = path
        .parent()
        .and_then(vsg_rs::config::discover)
        .and_then(|file| Config::load(&[file]).ok());
    let cfg = found.unwrap_or_default();
    cfg.for_path(path).into_owned()
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
        let diagnostics = tokio::task::spawn_blocking({
            let path = path.clone();
            let text = text.clone();
            move || diagnose(&path, &text)
        })
        .await
        .unwrap_or_default();
        // Analyses run concurrently and do not finish in the order they started: a small edit can
        // overtake the larger buffer before it. Publishing that late result would leave the editor
        // showing diagnostics for a version the user has already moved past.
        if self
            .documents
            .read()
            .await
            .get(&uri)
            .is_some_and(|document| document.version > version)
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
fn diagnose(path: &Path, text: &str) -> Vec<Diagnostic> {
    let cfg = config_for(path);
    let parsed = vsg_rs::Parsed::new(text.as_bytes().to_vec());
    let mut out = Vec::new();

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

    for finding in analysis::findings_for(&parsed, path, &cfg) {
        let at = position_at(finding.line, finding.column);
        // Structured, not flattened into the message: an editor can jump to each one.
        let related: Vec<DiagnosticRelatedInformation> = finding
            .related
            .iter()
            .filter_map(|other| {
                let uri: Uri = format!("file://{}", other.file.display()).parse().ok()?;
                Some(DiagnosticRelatedInformation {
                    location: Location {
                        uri,
                        range: Range::new(
                            position_at(other.line, other.column),
                            position_at(other.line, other.column),
                        ),
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
                let cfg = config_for(&path);
                let parsed = vsg_rs::Parsed::new(source);
                vsg_rs::fix_with(&parsed, &cfg, &vsg_rs::FixOptions::default())
                    .ok()
                    .map(|out| out.output)
            }
        })
        .await
        .ok()
        .flatten();
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

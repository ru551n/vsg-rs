//! `vsg-rs lsp` over the wire.
//!
//! These drive the real binary through stdin and stdout, because the point of a protocol server
//! is what it puts on the wire, not what its functions return.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Switch a class of lint rules on for everything under `dir`, the way a project does it: a
/// `vsg-rs.yaml` the server discovers by walking up from the file.
///
/// Only definite errors run unless asked for. Most of these tests are about how a finding
/// reaches the editor, and the advisory rules make the most convenient findings, so they say so.
fn enable_class(dir: &std::path::Path, class: &str) {
    std::fs::write(
        dir.join("vsg-rs.yaml"),
        format!("rule:\n  group:\n    {class}:\n      disable: false\n"),
    )
    .expect("write config");
}

fn frame(body: &serde_json::Value) -> Vec<u8> {
    let body = body.to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

/// A running server. Messages are sent, then stdout is read until what the test is waiting for
/// arrives -- diagnostics are published from a worker thread, so they do not arrive with the
/// response to the request that caused them.
struct Session {
    child: Child,
    reader: std::sync::mpsc::Receiver<u8>,
    /// Whether the handshake has been done, so a second exchange does not repeat it.
    started: bool,
    /// Everything read so far. It lives on the session, because a read that stops at one message
    /// must not throw away the bytes of the next.
    raw: Vec<u8>,
}

impl Session {
    fn start() -> Session {
        Session::start_in(&std::env::current_dir().expect("a working directory"))
    }

    fn start_in(dir: &std::path::Path) -> Session {
        let mut child = Command::new(env!("CARGO_BIN_EXE_vsg-rs"))
            .arg("lsp")
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn vsg-rs lsp");
        let mut stdout = child.stdout.take().expect("stdout");
        let (send, reader) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut byte = [0u8; 1];
            while stdout.read(&mut byte).unwrap_or(0) == 1 {
                if send.send(byte[0]).is_err() {
                    break;
                }
            }
        });
        Session {
            child,
            reader,
            raw: Vec::new(),
            started: false,
        }
    }

    fn send(&mut self, message: &serde_json::Value) {
        let stdin = self.child.stdin.as_mut().expect("stdin");
        stdin.write_all(&frame(message)).expect("write");
        stdin.flush().expect("flush");
    }

    /// Read until `done` is satisfied, or time runs out.
    fn read_while(
        &mut self,
        done: impl Fn(&[serde_json::Value]) -> bool,
    ) -> Vec<serde_json::Value> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let seen = parse(&self.raw);
            if done(&seen) || std::time::Instant::now() >= deadline {
                return seen;
            }
            match self
                .reader
                .recv_timeout(std::time::Duration::from_millis(50))
            {
                Ok(byte) => self.raw.push(byte),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return parse(&self.raw),
            }
        }
    }

    /// Initialize, send the rest, then read until `done`.
    fn talk_while(
        &mut self,
        messages: &[serde_json::Value],
        done: impl Fn(&[serde_json::Value]) -> bool,
    ) -> Vec<serde_json::Value> {
        if !self.started {
            self.started = true;
            self.send(&initialize());
            self.read_until(1);
            self.send(&serde_json::json!({
                "jsonrpc": "2.0", "method": "initialized", "params": {}
            }));
        }
        for message in messages {
            self.send(message);
        }
        self.read_while(done)
    }

    /// Everything that arrives in the next `millis`, however little that is.
    ///
    /// For asserting that something does *not* happen: waiting on a condition that never comes
    /// true would burn the whole deadline on every run.
    fn drain(&mut self, millis: u64) -> Vec<serde_json::Value> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(millis);
        while std::time::Instant::now() < deadline {
            match self
                .reader
                .recv_timeout(std::time::Duration::from_millis(20))
            {
                Ok(byte) => self.raw.push(byte),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        parse(&self.raw)
    }

    /// Read until `wanted` messages have arrived in total, or time runs out.
    fn read_until(&mut self, wanted: usize) -> Vec<serde_json::Value> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let seen = parse(&self.raw);
            // A server's log lines are not answers; waiting for a count of everything would stop
            // as soon as it said hello.
            let answers = seen
                .iter()
                .filter(|m| m["method"] != "window/logMessage")
                .count();
            if answers >= wanted || std::time::Instant::now() >= deadline {
                return seen;
            }
            match self
                .reader
                .recv_timeout(std::time::Duration::from_millis(100))
            {
                Ok(byte) => self.raw.push(byte),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return parse(&self.raw),
            }
        }
    }

    /// Initialize, then send the rest. A client must not send anything before the initialize
    /// response arrives, and a server is entitled to ignore what it gets too early -- so a test
    /// that pipelines everything is testing a client nobody writes.
    fn talk(&mut self, messages: &[serde_json::Value], wanted: usize) -> Vec<serde_json::Value> {
        self.send(&initialize());
        self.read_until(1);
        self.send(&serde_json::json!({
            "jsonrpc": "2.0", "method": "initialized", "params": {}
        }));
        for message in messages {
            self.send(message);
        }
        // `wanted` counts everything, including the initialize response.
        self.read_until(wanted + 1)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The complete messages in `data`; a trailing partial one is ignored, because this is called
/// while the server is still writing.
fn parse(mut data: &[u8]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    while let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
        let header = String::from_utf8_lossy(&data[..at]).to_ascii_lowercase();
        let Some(length) = header
            .split("content-length:")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|n| n.parse::<usize>().ok())
        else {
            break;
        };
        let Some(body) = data.get(at + 4..at + 4 + length) else {
            break;
        };
        match serde_json::from_slice(body) {
            Ok(message) => out.push(message),
            Err(_) => break,
        }
        data = &data[at + 4 + length..];
    }
    out
}

/// A file URI for a path, on any platform: `file:///home/x.vhd`, `file:///C:/dir/x.vhd`.
///
/// Percent-encoded, because that is what an editor sends: a raw space or `#` in a URI is not a
/// URI, and a test that sends one is testing a client nobody writes.
fn file_uri(path: &std::path::Path) -> String {
    let text = path.display().to_string().replace('\\', "/");
    let mut encoded = String::new();
    for c in text.chars() {
        if matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '/' | '-' | '.' | '_' | '~' | ':') {
            encoded.push(c);
        } else {
            let mut buffer = [0u8; 4];
            for b in c.encode_utf8(&mut buffer).as_bytes() {
                write!(encoded, "%{b:02X}").expect("a String never fails to write");
            }
        }
    }
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

fn initialize() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "capabilities": {}, "processId": null, "rootUri": null }
    })
}

fn did_open(uri: &str, text: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": uri, "languageId": "vhdl", "version": 1, "text": text
        }}
    })
}

const TWO_DRIVERS: &str = "entity dut is\n  port (\n    a : in  bit;\n    b : in  bit;\n    \
                           q : out bit\n  );\nend entity dut;\n\narchitecture rtl of dut is\n\n\
                           begin\n\n  q <= a;\n  q <= b;\n\nend architecture rtl;\n";

#[test]
fn it_does_not_advertise_being_a_vhdl_language_server() {
    let got = Session::start().talk(&[], 0);
    let result = &got
        .iter()
        .find(|m| m["id"] == 1)
        .expect("an initialize response")["result"];
    let capabilities = &result["capabilities"];

    assert_eq!(result["serverInfo"]["name"], "vsg-rs");
    // What vsg-rs is: diagnostics (through sync) and formatting.
    assert!(capabilities["textDocumentSync"].is_number());
    assert_eq!(capabilities["documentFormattingProvider"], true);
    let kinds = capabilities["codeActionProvider"]["codeActionKinds"]
        .as_array()
        .expect("the kinds it offers are declared");
    assert!(kinds.iter().any(|k| k == "quickfix"));
    assert!(kinds.iter().any(|k| k == "source.fixAll"));

    // What belongs to vhdl_ls. Advertising any of these would make editors ask vsg-rs for
    // answers it has no business giving.
    for capability in [
        "completionProvider",
        "hoverProvider",
        "definitionProvider",
        "declarationProvider",
        "typeDefinitionProvider",
        "implementationProvider",
        "referencesProvider",
        "renameProvider",
        "documentSymbolProvider",
        "workspaceSymbolProvider",
        "semanticTokensProvider",
        "signatureHelpProvider",
        "inlayHintProvider",
    ] {
        assert!(
            capabilities
                .get(capability)
                .is_none_or(serde_json::Value::is_null),
            "{capability} must not be advertised"
        );
    }
}

#[test]
fn diagnostics_carry_related_locations() {
    // A real file: a related location names a file, and naming one that is not there is not a
    // thing an editor ever asks about.
    let dir = tempfile::tempdir().expect("tempdir");
    enable_class(dir.path(), "advisory");
    let file = dir.path().join("dut.vhd");
    std::fs::write(&file, TWO_DRIVERS).expect("write");
    let got = Session::start().talk(&[did_open(&file_uri(&file), TWO_DRIVERS)], 1);
    let published = got
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .unwrap_or_else(|| panic!("diagnostics were published; got {got:#?}"));
    assert_eq!(published["params"]["version"], 1);
    let drivers = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .find(|d| d["code"] == "lint_601")
        .expect("the multiple driver is reported");
    assert_eq!(drivers["source"], "vsg-rs");
    // Structured, so an editor can offer each one as a place to go.
    let related = drivers["relatedInformation"]
        .as_array()
        .expect("relatedInformation");
    assert_eq!(related.len(), 2);
    assert!(related[0]["location"]["uri"].is_string());
}

#[test]
fn an_edit_replaces_the_diagnostics_of_the_version_before_it() {
    let edited = TWO_DRIVERS.replace("  q <= b;\n", "");
    // Wait for the edit's own diagnostics: the open's may never be published at all, because a
    // result that a newer version has overtaken is dropped rather than shown.
    let got = Session::start().talk_while(
        &[
            did_open("file:///tmp/dut.vhd", TWO_DRIVERS),
            serde_json::json!({
                "jsonrpc": "2.0", "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": "file:///tmp/dut.vhd", "version": 2 },
                    "contentChanges": [{ "text": edited }]
                }
            }),
        ],
        |seen| {
            seen.iter().any(|m| {
                m["method"] == "textDocument/publishDiagnostics" && m["params"]["version"] == 2
            })
        },
    );
    let published: Vec<&serde_json::Value> = got
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics")
        .collect();
    let last = published.last().expect("diagnostics were published");
    assert_eq!(
        last["params"]["version"], 2,
        "the newest version wins; got {published:#?}"
    );
    let lint: Vec<&serde_json::Value> = last["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter(|d| d["code"].as_str().is_some_and(|c| c.starts_with("lint_")))
        .collect();
    assert!(
        lint.is_empty(),
        "the buffer no longer has two drivers: {lint:?}"
    );
}

/// A project with a library map, so the rules that resolve names across files can run —
/// reached through a symlink where the platform allows one.
///
/// An editor sends the path the user opened, while the project knows the path its library map
/// resolved to. Those differ whenever any directory on the way is a symlink, which on macOS is
/// every temporary directory, and a buffer updated under the wrong one is silently never
/// analysed. Reaching the fixture through a link makes every platform test that.
///
/// The returned directory guard must outlive the path.
fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = dir.path().join("project");
    std::fs::create_dir_all(real.join("src")).expect("mkdir");
    // lint_004 is advisory: an unused declaration is legal VHDL. These tests are about the
    // buffer reaching the analysis, so they ask for it.
    enable_class(&real, "advisory");
    std::fs::write(
        real.join("vhdl_ls.toml"),
        "[libraries]\nmylib.files = [\"src/*.vhd\"]\n",
    )
    .expect("write config");
    let entry = reached_through_a_link(&real, &dir.path().join("link"));
    (dir, entry)
}

/// `real`, reached through `link`, or `real` itself where a symlink cannot be created —
/// on Windows that needs a privilege the machine may not grant, and macOS covers the case.
#[cfg(unix)]
fn reached_through_a_link(real: &Path, link: &Path) -> PathBuf {
    match std::os::unix::fs::symlink(real, link) {
        Ok(()) => link.to_path_buf(),
        Err(_) => real.to_path_buf(),
    }
}

#[cfg(not(unix))]
fn reached_through_a_link(real: &Path, _link: &Path) -> PathBuf {
    real.to_path_buf()
}

#[test]
fn the_front_ends_rules_reach_the_editor_too() {
    let (_dir, dir) = project();
    let file = dir.join("src/e.vhd");
    let source = "entity e is\nend entity e;\n\narchitecture rtl of e is\n\n  \
                  signal spare : bit;\n\nbegin\n\nend architecture rtl;\n";
    // The file on disk does not carry the signal, so only the buffer can be the source of the
    // finding below. Writing the same text to both would let a project that never saw the
    // buffer pass this test on the strength of the file.
    std::fs::write(
        &file,
        "entity e is\nend entity e;\n\narchitecture rtl of e is\n\nbegin\n\nend architecture rtl;\n",
    )
    .expect("write source");

    let uri = file_uri(&file);
    let mut session = Session::start_in(&dir);
    let got = session.talk_while(&[did_open(&uri, source)], |seen| {
        seen.iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics")
    });
    let published = got
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .expect("diagnostics were published");
    let codes: Vec<&str> = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    // lint_004 comes from the VHDL front end, not from vsg-rs's own rules: an editor sees what
    // `--check style,lint` sees, not a subset of it.
    assert!(codes.contains(&"lint_004"), "{codes:?}");
}

#[test]
fn the_kept_project_follows_the_buffer() {
    // The analysed project is kept between edits rather than rebuilt, which is what makes an
    // editor answer quickly. A cache that goes stale would be worse than a slow one: these
    // assert that a finding appears when an edit creates it and goes when an edit removes it.
    let (_dir, dir) = project();
    let file = dir.join("src/e.vhd");
    let clean = "entity e is\nend entity e;\n\narchitecture rtl of e is\n\nbegin\n\n\
                 end architecture rtl;\n";
    let with_spare = "entity e is\nend entity e;\n\narchitecture rtl of e is\n\n  \
                      signal spare : bit;\n\nbegin\n\nend architecture rtl;\n";
    std::fs::write(&file, clean).expect("write source");

    let uri = file_uri(&file);
    let mut session = Session::start_in(&dir);
    let got = session.talk_while(
        &[
            did_open(&uri, clean),
            // An edit that introduces an unused signal.
            serde_json::json!({
                "jsonrpc": "2.0", "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": uri, "version": 2 },
                    "contentChanges": [{ "text": with_spare }]
                }
            }),
        ],
        |seen| {
            seen.iter().any(|m| {
                m["method"] == "textDocument/publishDiagnostics" && m["params"]["version"] == 2
            })
        },
    );
    let version = |n: i64| {
        got.iter()
            .filter(|m| m["method"] == "textDocument/publishDiagnostics")
            .find(|m| m["params"]["version"] == n)
            .map(|m| m["params"]["diagnostics"].to_string())
    };
    assert!(
        version(2).expect("the edit was analysed").contains("spare"),
        "a signal the edit added is reported"
    );

    // And back again: the finding goes when the edit that caused it does.
    let got = session.talk_while(
        &[serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": uri, "version": 3 },
                "contentChanges": [{ "text": clean }]
            }
        })],
        |seen| {
            seen.iter().any(|m| {
                m["method"] == "textDocument/publishDiagnostics" && m["params"]["version"] == 3
            })
        },
    );
    let last = got
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics")
        .find(|m| m["params"]["version"] == 3)
        .expect("the third version was analysed");
    assert!(
        !last["params"]["diagnostics"].to_string().contains("spare"),
        "the finding goes with the signal: {last}"
    );
}

#[test]
fn quick_fixes_come_from_the_fix_the_finding_already_carries() {
    let source = "entity e is\nend;\n";
    let got = Session::start().talk_while(
        &[
            did_open("file:///tmp/qf.vhd", source),
            serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "textDocument/codeAction",
                "params": {
                    "textDocument": { "uri": "file:///tmp/qf.vhd" },
                    "range": {
                        "start": { "line": 1, "character": 0 },
                        "end": { "line": 1, "character": 0 }
                    },
                    "context": { "diagnostics": [] }
                }
            }),
        ],
        |seen| seen.iter().any(|m| m["id"] == 2),
    );
    let actions = got
        .iter()
        .find(|m| m["id"] == 2)
        .expect("a code action response")["result"]
        .as_array()
        .expect("actions")
        .clone();

    let quick: Vec<&serde_json::Value> =
        actions.iter().filter(|a| a["kind"] == "quickfix").collect();
    assert!(!quick.is_empty(), "the cursor is on a fixable finding");

    // Applying one produces what that rule's fix means, not a whole reformat.
    let edits = quick[0]["edit"]["changes"]["file:///tmp/qf.vhd"]
        .as_array()
        .expect("edits");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0]["newText"], " entity");

    // Fix-all is offered separately, and is the whole document.
    let fix_all: Vec<&serde_json::Value> = actions
        .iter()
        .filter(|a| a["kind"] == "source.fixAll")
        .collect();
    assert_eq!(fix_all.len(), 1, "{actions:#?}");
    let whole = fix_all[0]["edit"]["changes"]["file:///tmp/qf.vhd"]
        .as_array()
        .expect("edits");
    assert_eq!(whole.len(), 1, "one edit for the document");
    assert_eq!(whole[0]["newText"], "entity e is\nend entity e;\n");
}

#[test]
fn fix_all_applies_only_what_the_command_line_would_apply() {
    // `a : bit` has an unsafe fix (adding the `in` mode changes an interface), which `--fix`
    // leaves alone without `--unsafe_fixes`. Fix-all must leave it alone too.
    let source = "entity e is\n  port (a : bit);\nend entity e;\n";
    let got = Session::start().talk_while(
        &[
            did_open("file:///tmp/unsafe.vhd", source),
            serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "textDocument/codeAction",
                "params": {
                    "textDocument": { "uri": "file:///tmp/unsafe.vhd" },
                    "range": {
                        "start": { "line": 1, "character": 9 },
                        "end": { "line": 1, "character": 9 }
                    },
                    "context": { "diagnostics": [], "only": ["source.fixAll"] }
                }
            }),
        ],
        |seen| seen.iter().any(|m| m["id"] == 2),
    );
    let actions = got
        .iter()
        .find(|m| m["id"] == 2)
        .expect("a code action response")["result"]
        .as_array()
        .expect("actions")
        .clone();
    assert!(
        actions.iter().all(|a| a["kind"] == "source.fixAll"),
        "only what was asked for: {actions:#?}"
    );
    for action in &actions {
        let edits = action["edit"]["changes"]["file:///tmp/unsafe.vhd"]
            .as_array()
            .expect("edits");
        let text = edits[0]["newText"].as_str().expect("text");
        assert!(
            !text.contains("in    bit"),
            "an unsafe fix must not be applied by fix-all: {text:?}"
        );
    }
}

#[test]
fn formatting_is_what_the_command_line_would_have_written() {
    let source = "entity e is port (a : in bit); end;\n";
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("e.vhd");
    std::fs::write(&file, source).expect("write");
    // What `--fix` produces, which is what CI checks.
    let fixed = Command::new(env!("CARGO_BIN_EXE_vsg-rs"))
        .args([file.to_str().unwrap(), "--fix"])
        .output()
        .expect("run --fix");
    assert!(fixed.status.success() || fixed.status.code() == Some(1));
    let expected = std::fs::read_to_string(&file).expect("read back");

    let uri = file_uri(&file);
    let got = Session::start().talk(
        &[
            did_open(&uri, source),
            serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "textDocument/formatting",
                "params": {
                    "textDocument": { "uri": uri },
                    "options": { "tabSize": 2, "insertSpaces": true }
                }
            }),
        ],
        2,
    );
    let edits = got
        .iter()
        .find(|m| m["id"] == 2)
        .expect("a formatting response")["result"]
        .as_array()
        .expect("edits")
        .clone();
    assert_eq!(edits.len(), 1, "one edit for the whole document");
    assert_eq!(
        edits[0]["newText"].as_str().expect("new text"),
        expected,
        "the editor and the command line must not disagree"
    );
}

#[test]
fn a_closed_document_does_not_get_its_diagnostics_back() {
    // Closing withdraws the diagnostics, but an analysis started before the close is still
    // running. Publishing its result afterwards would put the problems back for a file the
    // editor is no longer showing.
    let mut session = Session::start();
    let got = session.talk_while(
        &[
            did_open("file:///tmp/closing.vhd", TWO_DRIVERS),
            serde_json::json!({
                "jsonrpc": "2.0", "method": "textDocument/didClose",
                "params": { "textDocument": { "uri": "file:///tmp/closing.vhd" } }
            }),
        ],
        |seen| {
            // The withdrawal `did_close` sends itself: an empty set with no version.
            seen.iter().any(|m| {
                m["method"] == "textDocument/publishDiagnostics"
                    && m["params"]["diagnostics"]
                        .as_array()
                        .is_some_and(Vec::is_empty)
            })
        },
    );
    assert!(!got.is_empty(), "the server answered");

    // Whatever else arrives, none of it may put diagnostics back.
    // Long enough for the analysis that was in flight to finish: the first one builds the
    // `ieee` and `std` libraries, so a few hundred milliseconds is not enough to prove anything.
    let after = session.drain(4000);
    let published: Vec<&serde_json::Value> = after
        .iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics")
        .collect();
    let last = published.last().expect("something was published");
    assert!(
        last["params"]["diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "the last word on a closed document is that it has no diagnostics: {published:#?}"
    );
}

#[test]
fn related_locations_survive_a_path_that_needs_encoding() {
    // `format!("file://{path}")` is not a URI. A space or a `#` made the parse fail, and the
    // related location was quietly dropped -- so a multiple-driver finding lost the other
    // driver depending on what the directory was called.
    let dir = tempfile::tempdir().expect("tempdir");
    enable_class(dir.path(), "advisory");
    let awkward = dir.path().join("my design #2");
    std::fs::create_dir_all(&awkward).expect("mkdir");
    let file = awkward.join("dut.vhd");
    std::fs::write(&file, TWO_DRIVERS).expect("write");

    let uri = file_uri(&file);
    let got = Session::start().talk(&[did_open(&uri, TWO_DRIVERS)], 1);
    let published = got
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .unwrap_or_else(|| panic!("diagnostics were published; got {got:#?}"));
    let drivers = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .find(|d| d["code"] == "lint_601")
        .expect("the multiple driver is reported");
    let related = drivers["relatedInformation"]
        .as_array()
        .expect("relatedInformation");
    assert_eq!(related.len(), 2, "both drivers are named: {related:#?}");
    let named = related[0]["location"]["uri"].as_str().expect("a uri");
    assert!(named.starts_with("file://"), "{named}");
    assert!(
        !named.contains(' '),
        "a URI never contains a raw space: {named}"
    );
}

#[test]
fn a_configuration_that_does_not_load_stops_the_editor_rather_than_defaulting() {
    // Silently falling back to the defaults let format-on-save rewrite a file under settings
    // the project never chose, while the command line refused to run at all.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("vsg-rs.yaml"),
        "rule: [this is not a mapping\n",
    )
    .expect("write config");
    let file = dir.path().join("dut.vhd");
    std::fs::write(&file, TWO_DRIVERS).expect("write source");

    let uri = file_uri(&file);
    let mut session = Session::start_in(dir.path());
    let got = session.talk_while(&[did_open(&uri, TWO_DRIVERS)], |seen| {
        seen.iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics")
    });
    let published = got
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .expect("diagnostics were published");
    let diagnostics = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics");
    assert_eq!(
        diagnostics.len(),
        1,
        "only the configuration: {diagnostics:#?}"
    );
    let message = diagnostics[0]["message"].as_str().expect("a message");
    assert!(
        message.contains("configuration could not be read"),
        "{message}"
    );
    assert!(message.contains("vsg-rs.yaml"), "names the file: {message}");

    // And it refuses to format, rather than formatting with the defaults.
    let answered = session.talk_while(
        &[serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "textDocument/formatting",
            "params": {
                "textDocument": { "uri": uri },
                "options": { "tabSize": 2, "insertSpaces": true }
            }
        })],
        |seen| seen.iter().any(|m| m["id"] == 2),
    );
    let reply = answered
        .iter()
        .find(|m| m["id"] == 2)
        .expect("the request was answered");
    assert!(
        reply["error"].is_object(),
        "formatting is refused, not done with the defaults: {reply:#?}"
    );
}

#[test]
fn an_unreadable_library_map_says_why_the_resolving_rules_are_quiet() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("src")).expect("mkdir");
    std::fs::write(dir.path().join("vhdl_ls.toml"), "[libraries\nbroken = =\n")
        .expect("write config");
    let file = dir.path().join("src/dut.vhd");
    std::fs::write(&file, TWO_DRIVERS).expect("write source");

    let uri = file_uri(&file);
    let got = Session::start_in(dir.path()).talk_while(&[did_open(&uri, TWO_DRIVERS)], |seen| {
        seen.iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics")
    });
    let published = got
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .expect("diagnostics were published");
    let messages: Vec<&str> = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter_map(|d| d["message"].as_str())
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("library map could not be read")),
        "a report quietly missing most of its rules looks like a clean one: {messages:#?}"
    );
}

#[test]
fn a_finding_after_a_tab_points_at_the_right_character() {
    // A rule working from the syntax tree reports a display column: the tab counts four. LSP
    // counts UTF-16 code units, where it counts one, so the marker used to sit three characters
    // to the right of the signal it is about. The comment adds a non-BMP character, which LSP
    // counts as two and a display column as one -- the same disagreement the other way.
    let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n  \
                  -- \u{1f980}\n  signal q : bit;\nbegin\n\tq <= '1';\n\tq <= '0';\n\
                  end architecture rtl;\n";
    let dir = tempfile::tempdir().expect("tempdir");
    enable_class(dir.path(), "advisory");
    let file = dir.path().join("tabbed.vhd");
    std::fs::write(&file, source).expect("write");
    let got = Session::start().talk(&[did_open(&file_uri(&file), source)], 1);
    let published = got
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .unwrap_or_else(|| panic!("diagnostics were published; got {got:#?}"));
    let drivers = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .find(|d| d["code"] == "lint_601")
        .expect("the multiple driver is reported");

    // Line 8 (zero-based 7) is "\tq <= '1';": the tab is character 0, `q` is character 1.
    assert_eq!(drivers["range"]["start"]["line"], 7, "{drivers:#?}");
    assert_eq!(
        drivers["range"]["start"]["character"], 1,
        "one tab is one UTF-16 code unit, not four columns: {drivers:#?}"
    );
    // The other driver is a related location, in this same buffer, and counts the same way.
    let related = drivers["relatedInformation"]
        .as_array()
        .expect("relatedInformation");
    let lines: Vec<i64> = related
        .iter()
        .filter_map(|r| r["location"]["range"]["start"]["line"].as_i64())
        .collect();
    assert!(lines.contains(&7) && lines.contains(&8), "{related:#?}");
    for r in related {
        assert_eq!(
            r["location"]["range"]["start"]["character"], 1,
            "related locations count the same way: {r:#?}"
        );
    }
}

/// A project under `dir` whose library map is `library`, holding one file with an unused signal.
fn project_named(dir: &std::path::Path, library: &str) -> PathBuf {
    std::fs::create_dir_all(dir.join("src")).expect("mkdir");
    enable_class(dir, "advisory");
    std::fs::write(
        dir.join("vhdl_ls.toml"),
        format!("[libraries]\n{library}.files = [\"src/*.vhd\"]\n"),
    )
    .expect("write config");
    let file = dir.join("src/e.vhd");
    std::fs::write(
        &file,
        "entity e is\nend entity e;\n\narchitecture rtl of e is\n\n  signal spare : bit;\n\n\
         begin\n\nend architecture rtl;\n",
    )
    .expect("write source");
    file
}

/// The rule codes published for `uri` after opening it with `text`.
fn codes_for(session: &mut Session, uri: &str, text: &str) -> Vec<String> {
    let got = session.talk_while(&[did_open(uri, text)], |seen| {
        seen.iter()
            .any(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri)
    });
    got.iter()
        .filter(|m| m["method"] == "textDocument/publishDiagnostics" && m["params"]["uri"] == uri)
        .filter_map(|m| m["params"]["diagnostics"].as_array())
        .flatten()
        .filter_map(|d| d["code"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn two_projects_in_one_server_are_analysed_as_two_projects() {
    // One analyser for the whole server meant the second project was resolved against the
    // first one's library map. An editor with two folders open is the ordinary case.
    let one = tempfile::tempdir().expect("tempdir");
    let two = tempfile::tempdir().expect("tempdir");
    let first = project_named(one.path(), "alpha");
    let second = project_named(two.path(), "beta");

    let source = std::fs::read_to_string(&first).expect("read");
    let mut session = Session::start_in(one.path());
    let a = codes_for(&mut session, &file_uri(&first), &source);
    let b = codes_for(&mut session, &file_uri(&second), &source);
    // lint_004 needs the file's own library map. Both files have one, so both get it.
    assert!(a.contains(&"lint_004".to_owned()), "first project: {a:?}");
    assert!(
        b.contains(&"lint_004".to_owned()),
        "the second project has its own map and is not answered from the first: {b:?}"
    );
}

#[test]
fn editing_the_library_map_rebuilds_the_project() {
    // The project used to be built once and kept for the life of the server, so a library map
    // that changed was believed until someone restarted the editor.
    let dir = tempfile::tempdir().expect("tempdir");
    let file = project_named(dir.path(), "mylib");
    let source = std::fs::read_to_string(&file).expect("read");
    // Start with a map that covers nothing: the file is in no library, so the rules that
    // resolve names across files cannot run.
    std::fs::write(
        dir.path().join("vhdl_ls.toml"),
        "[libraries]\nmylib.files = [\"elsewhere/*.vhd\"]\n",
    )
    .expect("rewrite config");
    // Both files exist on disk before anything is analysed, so the only thing that changes
    // between the two answers is the map.
    let other = dir.path().join("src/other.vhd");
    std::fs::write(
        &other,
        source
            .replace("entity e", "entity o")
            .replace("of e", "of o"),
    )
    .expect("write source");

    let mut session = Session::start_in(dir.path());
    let before = codes_for(&mut session, &file_uri(&file), &source);
    assert!(
        !before.contains(&"lint_004".to_owned()),
        "nothing is in a library yet: {before:?}"
    );

    std::fs::write(
        dir.path().join("vhdl_ls.toml"),
        "[libraries]\nmylib.files = [\"src/*.vhd\"]\n",
    )
    .expect("rewrite config");
    let after = codes_for(
        &mut session,
        &file_uri(&other),
        &std::fs::read_to_string(&other).expect("read"),
    );
    assert!(
        after.contains(&"lint_004".to_owned()),
        "the rewritten map is read, not the one from start-up: {after:?}"
    );
}

#[test]
fn a_definite_error_reaches_the_editor_as_the_command_line_sees_it() {
    // The contract for a new rule: no LSP-specific analysis, so an unsaved buffer gets exactly
    // what the command line would report for the same bytes. lint_770 is a default rule, so
    // this needs no configuration at all.
    let dir = tempfile::tempdir().expect("tempdir");
    let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n  \
                  signal x : bit;\nbegin\n  p : process\n  begin\n    x <= '1';\n  \
                  end process p;\nend architecture rtl;\n";
    let file = dir.path().join("dut.vhd");
    std::fs::write(&file, source).expect("write");

    let got = Session::start().talk(&[did_open(&file_uri(&file), source)], 1);
    let published = got
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .unwrap_or_else(|| panic!("diagnostics were published; got {got:#?}"));
    let stuck = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .find(|d| d["code"] == "lint_770")
        .expect("the process that cannot suspend is reported");
    // Line 7 in the source, zero-based 6.
    assert_eq!(stuck["range"]["start"]["line"], 6, "{stuck:#?}");
    assert!(
        stuck["message"].as_str().is_some_and(|m| m.contains("'p'")),
        "{stuck:#?}"
    );
}

//! `vsg-rs mcp`: the same analysis, answered over the Model Context Protocol.
//!
//! A coding agent editing VHDL wants what an editor wants: what is wrong with this buffer, and
//! what the formatter would do to it. The language server already answers both, but only to a
//! client that speaks LSP. This serves the same answers to a client that speaks MCP, from the
//! same library entry points, so an agent and a person are told the same thing about the same
//! file.
//!
//! It offers only what the tool can stand behind: linting, formatting, and what a rule means.
//!
//! # Transport
//!
//! stdio, as the specification defines it: one JSON-RPC message per line, no embedded newlines,
//! and **nothing on stdout that is not an MCP message**. Everything else goes to stderr, which
//! the client is free to ignore.
//!
//! # Protocol era
//!
//! The 2026-07-28 revision carries the protocol version per request and discovers a server with
//! `server/discover`; revisions before it opened with an `initialize` handshake. Both are
//! answered here, and no request depends on a handshake having happened, so a client of either
//! era is served.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The revision this server speaks.
const VERSION: &str = "2026-07-28";

/// A result that is the whole answer, cached by nobody.
///
/// `resultType`, `cacheScope` and `ttlMs` are required of every result by the schema.
fn complete(body: &Value) -> Value {
    let mut body = body.as_object().cloned().unwrap_or_default();
    body.insert("resultType".to_owned(), json!("complete"));
    body.insert("cacheScope".to_owned(), json!("private"));
    body.insert("ttlMs".to_owned(), json!(0));
    Value::Object(body)
}

/// What this server can be asked to do.
fn tools() -> Value {
    json!([
        {
            "name": "lint",
            "description": "Check VHDL and return what vsg-rs reports about it: syntax errors, \
                            style violations and lint findings, each with a rule id, a position \
                            and a message. The same answer `vsg-rs --check style,lint` gives for \
                            the same bytes. Pass `path` alone to check a file, or `source` to \
                            check a buffer before it is written.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file to check. With `source`, it is what that \
                                        source is called rather than what is read."
                    },
                    "source": {
                        "type": "string",
                        "description": "VHDL to check in place of the file, for source that is \
                                        not written yet or is being edited."
                    }
                },
                "anyOf": [{ "required": ["path"] }, { "required": ["source"] }]
            }
        },
        {
            "name": "format",
            "description": "Format VHDL as vsg-rs would write it: the project's layout, with \
                            every safe rule fix applied, which is exactly what `vsg-rs --fix` \
                            writes. Pass `source` to get the formatted text back, or `path` \
                            with `write` to fix a file in place without moving it through this \
                            conversation. Source that does not parse comes back unchanged, with \
                            the reason, and is never written.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file to format. With `source`, it is what that \
                                        source is called rather than what is read."
                    },
                    "source": {
                        "type": "string",
                        "description": "VHDL to format in place of the file, for source that is \
                                        not written yet or is being edited."
                    },
                    "write": {
                        "type": "boolean",
                        "description": "Write the result to `path` instead of returning it. \
                                        Defaults to false; nothing is written unless asked. A \
                                        file already formatted is left untouched.",
                        "default": false
                    }
                },
                "anyOf": [{ "required": ["path"] }, { "required": ["source"] }]
            }
        },
        {
            "name": "explain_rule",
            "description": "What a rule id means: its description, how sure it is, and whether \
                            a default run uses it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rule": {
                        "type": "string",
                        "description": "A rule id, such as `lint_740` or `entity_019`."
                    }
                },
                "required": ["rule"]
            }
        }
    ])
}

/// What a call is about: the source to work on, and what it is called.
///
/// Either the caller passes the source, and `path` only names it, or it passes a path alone and
/// the file is read. The second is for source already on disk, where sending the text there and
/// back is the expensive part of the answer.
fn subject(arguments: &Value) -> Result<(String, PathBuf), String> {
    let named = arguments["path"].as_str().unwrap_or_default();
    if let Some(source) = arguments["source"].as_str() {
        // An unnamed buffer still needs a name, because the configuration is found from one.
        let path = if named.is_empty() {
            "buffer.vhd"
        } else {
            named
        };
        return Ok((source.to_owned(), PathBuf::from(path)));
    }
    if named.is_empty() {
        return Err("give me either the source to work on or the path of a file".to_owned());
    }
    let path = PathBuf::from(named);
    let source = std::fs::read_to_string(&path).map_err(|e| format!("{named}: {e}"))?;
    Ok((source, path))
}

/// The configuration that applies to a name, found the way every other entry point finds it.
fn config_for(path: &Path) -> Result<vsg_rs::Config, String> {
    let Some(file) = path.parent().and_then(vsg_rs::config::discover) else {
        return Ok(vsg_rs::Config::default().for_path(path).into_owned());
    };
    let cfg = vsg_rs::Config::load(std::slice::from_ref(&file))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(cfg.for_path(path).into_owned())
}

/// Everything vsg-rs reports about one buffer, as data.
fn lint(source: &str, path: &Path) -> Result<Value, String> {
    let cfg = config_for(path)?;
    let parsed = vsg_rs::Parsed::new(source.as_bytes().to_vec());
    let at = |offset: usize| {
        let (line, column) = parsed.line_col(offset);
        json!({ "line": line, "column": column })
    };

    let mut found = Vec::new();
    // A file that does not parse gets its syntax errors and nothing else: every rule below
    // would be reasoning about a tree that does not represent the source.
    if !parsed.syntax_errors().is_empty() && !parsed.is_blank() {
        for error in parsed.syntax_errors() {
            found.push(json!({
                "rule": "syntax",
                "at": at(error.offset),
                "message": error.message,
            }));
        }
        return Ok(json!({ "findings": found, "parsed": false }));
    }

    for violation in vsg_rs::rules::check_with(&parsed, &cfg, None) {
        found.push(json!({
            "rule": violation.rule,
            "at": at(violation.start),
            "message": violation.message,
            "severity": violation.severity.to_string(),
        }));
    }
    // The front end's rules as well, so an agent sees what `--check style,lint` sees. They need
    // the project's library map, found from the file's path; without one only a few of them
    // report, exactly as on the command line.
    let (front_end, unbuilt) =
        match vsg_rs::analysis::lint::front_end_for(path, source.as_bytes().to_vec()) {
            Ok(findings) => (findings, None),
            Err(why) => (Vec::new(), Some(why)),
        };
    let native = vsg_rs::analysis::findings_for(&parsed, path, &cfg)
        .into_iter()
        .chain(front_end);
    for finding in native {
        if cfg.rule_by_id(finding.rule).is_some_and(|s| !s.enabled) {
            continue;
        }
        found.push(json!({
            "rule": finding.rule,
            "at": { "line": finding.line, "column": finding.column },
            "message": finding.message,
            "certainty": vsg_rs::analysis::certainty_of(finding.rule).map(Certainty::name),
        }));
    }
    let mut answer = json!({ "findings": found, "parsed": true });
    // Said out loud rather than left to silence: without the front end most of the lint layer
    // did not run, and a report missing it looks exactly like a clean one.
    if let Some(why) = unbuilt {
        answer["warning"] = json!(format!(
            "The rules that resolve names across files did not run: the project's library map \
             could not be read. {why}"
        ));
    }
    Ok(answer)
}

use vsg_rs::analysis::Certainty;

/// The source as `--fix` would write it, and on request written there.
fn format(source: &str, path: &Path, write: bool) -> Result<Value, String> {
    let cfg = config_for(path)?;
    let parsed = vsg_rs::Parsed::new(source.as_bytes().to_vec());
    let out = match vsg_rs::fix_with(&parsed, &cfg, &vsg_rs::FixOptions::default()) {
        Ok(out) => out,
        // Left exactly as it was, and why, which is what the command line does too. Nothing is
        // written: source that does not parse is the case where a formatter can do most harm.
        Err(e) => {
            return Ok(json!({ "source": source, "changed": false, "error": e.to_string() }));
        }
    };
    let text = String::from_utf8(out.output).map_err(|e| e.to_string())?;
    let changed = text != source;
    if !write {
        return Ok(json!({ "source": text, "changed": changed }));
    }
    // Nothing to write is not written: a file that is already formatted keeps its timestamp,
    // so a build that watches it does not rerun because a formatter looked at it.
    if changed {
        std::fs::write(path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    // The text is not sent back. Returning it is the whole cost this call exists to avoid.
    Ok(json!({ "changed": changed, "written": changed, "path": path.display().to_string() }))
}

/// What a rule reports, how sure it is, and whether a default run uses it.
fn explain(rule: &str) -> Result<Value, String> {
    let certainty = vsg_rs::analysis::certainty_of(rule);
    let description = vsg_rs::analysis::rules()
        .find(|known| known.id == rule)
        .map(|known| known.description.to_owned())
        .or_else(|| vsg_rs::rules::info(rule).map(|info| info.description.to_owned()))
        .ok_or_else(|| format!("no rule called '{rule}'"))?;
    Ok(json!({
        "rule": rule,
        "description": description,
        "certainty": certainty.map(Certainty::name),
        "default": certainty.map(|c| if c.on_by_default() { "on" } else { "off" }),
        "layer": if rule.starts_with("lint_") { "lint" } else { "style" },
    }))
}

/// Run one tool, as a `CallToolResult`.
fn call(name: &str, arguments: &Value) -> Value {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let write = arguments["write"].as_bool().unwrap_or(false);
    let answer = match name {
        "lint" => subject(arguments).and_then(|(source, path)| lint(&source, &path)),
        "format" => subject(arguments).and_then(|(source, path)| format(&source, &path, write)),
        "explain_rule" => explain(&text(&arguments["rule"])),
        other => Err(format!("no tool called '{other}'")),
    };
    // A tool that could not answer says so in its result rather than failing the call, which is
    // how the protocol asks for the client to be told.
    let (body, failed) = match answer {
        Ok(value) => (value.to_string(), false),
        Err(message) => (message, true),
    };
    complete(&json!({
        "content": [{ "type": "text", "text": body }],
        "isError": failed,
    }))
}

/// Answer one message, or `None` for a notification, which is not answered at all.
fn answer(message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message["method"].as_str().unwrap_or_default();
    let params = &message["params"];

    let result = match method {
        // The modern handshake: what this server is and which revisions it speaks.
        "server/discover" => complete(&json!({
            "supportedVersions": [VERSION],
            "capabilities": { "tools": {} },
            "_meta": {
                "io.modelcontextprotocol/serverInfo": {
                    "name": "vsg-rs",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            },
            "instructions": "Ask `lint` what is wrong with a VHDL buffer, `format` for what \
                             vsg-rs would write, and `explain_rule` what a rule id means.",
        })),
        // What revisions before 2026-07-28 opened with. Answered so a client of either era is
        // served; nothing here depends on it having happened.
        "initialize" => json!({
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or(VERSION),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "vsg-rs", "version": env!("CARGO_PKG_VERSION") },
        }),
        "tools/list" => complete(&json!({ "tools": tools() })),
        "tools/call" => call(
            params["name"].as_str().unwrap_or_default(),
            &params["arguments"],
        ),
        "ping" => complete(&json!({})),
        other => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("no method called '{other}'") },
            }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Serve MCP on stdin and stdout until the client closes the stream.
pub(crate) fn serve() -> std::process::ExitCode {
    let input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    for line in input.lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            eprintln!("vsg-rs mcp: ignoring a line that is not JSON");
            continue;
        };
        let Some(reply) = answer(&message) else {
            // A notification. Nothing is sent back, by definition.
            continue;
        };
        // One message per line, and never anything else on stdout.
        if writeln!(output, "{reply}").is_err() || output.flush().is_err() {
            break;
        }
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, params: &Value) -> Value {
        answer(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
            .expect("a request is answered")
    }

    /// The JSON a tool answered with.
    fn body_of(reply: &Value) -> Value {
        serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .expect("text"),
        )
        .expect("json")
    }

    #[test]
    fn a_notification_is_not_answered() {
        // No id, so there is nothing to answer to.
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(answer(&notification).is_none());
    }

    #[test]
    fn discover_says_what_it_speaks() {
        let reply = request("server/discover", &json!({}));
        assert_eq!(reply["result"]["supportedVersions"][0], VERSION);
        assert_eq!(reply["result"]["resultType"], "complete");
        assert!(reply["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn the_older_handshake_is_answered_too() {
        let reply = request("initialize", &json!({ "protocolVersion": "2025-06-18" }));
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(reply["result"]["serverInfo"]["name"], "vsg-rs");
    }

    #[test]
    fn every_tool_is_listed_with_a_schema() {
        let reply = request("tools/list", &json!({}));
        let listed = reply["result"]["tools"].as_array().expect("tools");
        assert_eq!(listed.len(), 3);
        for tool in listed {
            assert!(tool["name"].is_string(), "{tool}");
            assert!(tool["description"].is_string(), "{tool}");
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
        }
    }

    #[test]
    fn lint_reports_what_the_command_line_reports() {
        let reply = request(
            "tools/call",
            &json!({
                "name": "lint",
                "arguments": {
                    "source": "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n  \
                               signal x : bit;\nbegin\n  p : process\n  begin\n    x <= '1';\n  \
                               end process p;\nend architecture rtl;\n",
                    "path": "dut.vhd"
                }
            }),
        );
        assert_eq!(reply["result"]["isError"], false);
        let body = body_of(&reply);
        assert_eq!(body["parsed"], true);
        // lint_770: the process can never suspend. A default rule, so nothing to configure.
        assert!(
            body["findings"]
                .as_array()
                .expect("findings")
                .iter()
                .any(|f| f["rule"] == "lint_770" && f["certainty"] == "definite error"),
            "{body}"
        );
    }

    /// The front end is half the lint layer, and an answer missing it looks like a clean file.
    /// This asks for a rule only `vhdl_lang` can report, and one that needs no library map, so
    /// it holds wherever the tests run.
    ///
    /// A real file, because the front end analyses a buffer by standing it in for the file it
    /// names: a path that is on no disk belongs to no library and is never analysed.
    #[test]
    fn lint_runs_the_front_end_too() {
        let source = "entity dut is\nend entity dut;\n\narchitecture rtl of dut is\n  \
                      constant c : bit := '0';\n  signal a : bit;\n  signal b : bit;\nbegin\n  \
                      p : process (a, c) is\n  begin\n    b <= a;\n  end process p;\n\
                      end architecture rtl;\n";
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("dut.vhd");
        std::fs::write(&path, source).expect("the file is written");

        let reply = request(
            "tools/call",
            &json!({
                "name": "lint",
                "arguments": { "source": source, "path": path.to_str().expect("utf-8") }
            }),
        );
        let body = body_of(&reply);
        // lint_003: a constant is not a signal and cannot be in a sensitivity list.
        assert!(
            body["findings"]
                .as_array()
                .expect("findings")
                .iter()
                .any(|f| f["rule"] == "lint_003"),
            "{body}"
        );
    }

    #[test]
    fn format_returns_what_fix_would_write() {
        let reply = request(
            "tools/call",
            &json!({
                "name": "format",
                "arguments": { "source": "entity   dut is\nend entity dut;\n" }
            }),
        );
        let body = body_of(&reply);
        assert_eq!(body["changed"], true);
        assert!(
            body["source"]
                .as_str()
                .is_some_and(|s| s.contains("entity dut is")),
            "{body}"
        );
    }

    /// A file already on disk does not have to be sent here to be read.
    #[test]
    fn a_path_alone_is_read_from_disk() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("dut.vhd");
        std::fs::write(&path, "ENTITY   dut IS\nEND dut;\n").expect("the file is written");

        let reply = request(
            "tools/call",
            &json!({
                "name": "format",
                "arguments": { "path": path.to_str().expect("utf-8") }
            }),
        );
        let body = body_of(&reply);
        assert_eq!(body["changed"], true);
        assert!(
            body["source"]
                .as_str()
                .is_some_and(|s| s.starts_with("entity dut is")),
            "{body}"
        );
        // Asked for the text, not for the file to change.
        assert_eq!(
            std::fs::read_to_string(&path).expect("still readable"),
            "ENTITY   dut IS\nEND dut;\n"
        );
    }

    /// The point of writing in place: the file never travels through the conversation.
    #[test]
    fn write_fixes_the_file_and_returns_no_source() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("dut.vhd");
        std::fs::write(&path, "ENTITY   dut IS\nEND dut;\n").expect("the file is written");

        let arguments = json!({ "path": path.to_str().expect("utf-8"), "write": true });
        let body = body_of(&request(
            "tools/call",
            &json!({ "name": "format", "arguments": arguments }),
        ));
        assert_eq!(body["changed"], true);
        assert_eq!(body["written"], true);
        assert!(body["source"].is_null(), "{body}");
        let on_disk = std::fs::read_to_string(&path).expect("readable");
        assert_eq!(on_disk, "entity dut is\nend entity dut;\n");

        // Again, on a file that is already formatted: nothing to write, so nothing written.
        let before = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let body = body_of(&request(
            "tools/call",
            &json!({ "name": "format", "arguments": arguments }),
        ));
        assert_eq!(body["changed"], false);
        assert_eq!(body["written"], false);
        assert_eq!(
            std::fs::metadata(&path).and_then(|m| m.modified()).ok(),
            before,
            "an unchanged file kept its timestamp"
        );
    }

    /// The case where a formatter can do the most damage.
    #[test]
    fn source_that_does_not_parse_is_never_written() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("dut.vhd");
        let broken = "entity dut is\nend entity dut\n";
        std::fs::write(&path, broken).expect("the file is written");

        let body = body_of(&request(
            "tools/call",
            &json!({
                "name": "format",
                "arguments": { "path": path.to_str().expect("utf-8"), "write": true }
            }),
        ));
        assert_eq!(body["changed"], false);
        assert!(body["error"].is_string(), "{body}");
        assert_eq!(std::fs::read_to_string(&path).expect("readable"), broken);
    }

    #[test]
    fn a_call_with_neither_source_nor_path_says_so() {
        let reply = request("tools/call", &json!({ "name": "lint", "arguments": { } }));
        assert_eq!(reply["result"]["isError"], true);
    }

    #[test]
    fn source_that_does_not_parse_comes_back_unchanged() {
        let broken = "entity dut is\nend entity dut\n";
        let reply = request(
            "tools/call",
            &json!({ "name": "format", "arguments": { "source": broken } }),
        );
        let body = body_of(&reply);
        assert_eq!(body["source"], broken);
        assert_eq!(body["changed"], false);
        assert!(body["error"].is_string(), "{body}");
    }

    #[test]
    fn explain_says_how_sure_a_rule_is() {
        let reply = request(
            "tools/call",
            &json!({ "name": "explain_rule", "arguments": { "rule": "lint_712" } }),
        );
        let body = body_of(&reply);
        assert_eq!(body["certainty"], "advisory");
        assert_eq!(body["default"], "off");
    }

    #[test]
    fn a_tool_that_cannot_answer_says_so_in_its_result() {
        let reply = request(
            "tools/call",
            &json!({ "name": "explain_rule", "arguments": { "rule": "lint_999" } }),
        );
        assert_eq!(reply["result"]["isError"], true);
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|t| t.contains("lint_999")),
            "{reply}"
        );
    }

    #[test]
    fn an_unknown_method_is_a_protocol_error() {
        let reply = request("resources/list", &json!({}));
        assert_eq!(reply["error"]["code"], -32601);
    }
}

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
            "description": "Check VHDL source and return what vsg-rs reports about it: syntax \
                            errors, style violations and lint findings, each with a rule id, a \
                            position and a message. The same answer `vsg-rs --check style,lint` \
                            gives for the same bytes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "The VHDL source to check." },
                    "path": {
                        "type": "string",
                        "description": "What the source is called. Decides which configuration \
                                        applies. Optional."
                    }
                },
                "required": ["source"]
            }
        },
        {
            "name": "format",
            "description": "Return the source as vsg-rs would write it: the project's layout, \
                            with every safe rule fix applied, which is exactly what \
                            `vsg-rs --fix` writes. Source that does not parse comes back \
                            unchanged, with the reason.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "The VHDL source to format." },
                    "path": {
                        "type": "string",
                        "description": "What the source is called, which decides the \
                                        configuration. Optional."
                    }
                },
                "required": ["source"]
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
    for finding in vsg_rs::analysis::findings_for(&parsed, path, &cfg) {
        found.push(json!({
            "rule": finding.rule,
            "at": { "line": finding.line, "column": finding.column },
            "message": finding.message,
            "certainty": vsg_rs::analysis::certainty_of(finding.rule).map(Certainty::name),
        }));
    }
    Ok(json!({ "findings": found, "parsed": true }))
}

use vsg_rs::analysis::Certainty;

/// The source as `--fix` would write it.
fn format(source: &str, path: &Path) -> Result<Value, String> {
    let cfg = config_for(path)?;
    let parsed = vsg_rs::Parsed::new(source.as_bytes().to_vec());
    match vsg_rs::fix_with(&parsed, &cfg, &vsg_rs::FixOptions::default()) {
        Ok(out) => {
            let text = String::from_utf8(out.output).map_err(|e| e.to_string())?;
            let changed = text != source;
            Ok(json!({ "source": text, "changed": changed }))
        }
        // Left exactly as it was, and why, which is what the command line does too.
        Err(e) => Ok(json!({ "source": source, "changed": false, "error": e.to_string() })),
    }
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
    let path = || {
        let named = text(&arguments["path"]);
        if named.is_empty() {
            PathBuf::from("buffer.vhd")
        } else {
            PathBuf::from(named)
        }
    };
    let answer = match name {
        "lint" => lint(&text(&arguments["source"]), &path()),
        "format" => format(&text(&arguments["source"]), &path()),
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

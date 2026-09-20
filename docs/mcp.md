# MCP server

```sh
vsg-rs mcp
```

A [Model Context Protocol](https://modelcontextprotocol.io) server over stdin and stdout, so a
coding agent editing VHDL can ask what an editor asks: what is wrong with this buffer, and what
the formatter would write. Same rules, same configuration, same answers as the command line and
the [language server](lsp.md).

## Tools

| Tool | Arguments | Answers |
|---|---|---|
| `lint` | `path` or `source` | every finding, with rule id, line, column, message and class |
| `format` | `path` or `source`, and `write` | the formatted source, or what it changed when it wrote |
| `explain_rule` | `rule` | the rule's description, its class, and whether a default run uses it |

## A file, or source that is not one yet

`lint` and `format` take either. A `path` alone is read from disk. A `source` is used in place of
the file, with `path` naming it, which is how an agent checks what it is about to write before
writing it.

`format` returns the formatted text, unless `write` is true, in which case it writes the file and
returns only what changed. That is the point of it: formatting a file already on disk should not
cost the file twice over, once sent and once returned.

```json
{ "path": "rtl/fifo.vhd", "write": true }
```

Nothing is written unless `write` says so, and then only what parsed and actually changed. Source
with syntax errors comes back untouched, with the reason, and never reaches the file: that is
where a formatter can do the most damage. A file already formatted keeps its timestamp, so a
build watching it does not rerun because a formatter looked at it.

`path` is what the source is called. It decides two things, both found by walking up from that
name exactly as the command line does: the configuration that applies, and the project's
`vhdl_ls.toml`. An agent working in a configured project therefore gets that project's rules and
its whole lint layer, rather than the defaults and half of it.

Source that does not parse comes back from `format` unchanged, with the reason, and from `lint`
as its syntax errors alone.

## The buffer stands in for a file

The rules that resolve names across files analyse the buffer in place of the file it names, which
is how an unsaved edit is checked at all. That requires the file to exist: a path on no disk
belongs to no library, so those rules do not run for it. A buffer for a file that does exist is
fully analysed, edits and all.

When the library map cannot be read, `lint` says so in a `warning` field beside the findings
rather than returning a shorter list in silence. Most of the lint layer needs that map, and a
report quietly missing it looks exactly like a clean one.

## Configuring an agent

Claude Code:

```sh
claude mcp add vsg-rs -- vsg-rs mcp
```

Anything else that starts a server as a subprocess:

```json
{
  "mcpServers": {
    "vsg-rs": { "command": "vsg-rs", "args": ["mcp"] }
  }
}
```

## Protocol

The 2026-07-28 revision, answered statelessly: `server/discover`, `tools/list`, `tools/call`,
`ping`. The `initialize` handshake of earlier revisions is answered too, and nothing here depends
on a handshake having happened, so a client of either era works.

Transport is stdio as the specification defines it: one JSON-RPC message per line, and nothing on
stdout that is not an MCP message. Diagnostics go to stderr.

## A file called `mcp`

As with `lsp`, a file or directory named `mcp` in the working directory wins, and `vsg-rs mcp`
checks it. No VSG command line changes meaning.

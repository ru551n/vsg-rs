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
| `lint` | `source`, optional `path` | every finding, with rule id, line, column, message and class |
| `format` | `source`, optional `path` | the source as `--fix` would write it, and whether it changed |
| `explain_rule` | `rule` | the rule's description, its class, and whether a default run uses it |

`path` is what the source is called. It decides which configuration applies, found by walking up
from that name exactly as the command line does, so an agent working in a configured project gets
that project's rules rather than the defaults. Source that does not parse comes back from
`format` unchanged, with the reason, and from `lint` as its syntax errors alone.

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

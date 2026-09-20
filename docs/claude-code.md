# Claude Code plugin

```text
/plugin marketplace add ru551n/vsg-rs
/plugin install vsg-rs@vsg-rs
```

One install gives a coding agent both halves of what this tool is for: a **skill** that says when
and how to use it, and the [MCP server](mcp.md), registered and ready.

`vsg-rs` itself is not installed by the plugin. It has to be on `PATH`:

```sh
pip install vsg-rs        # or: uv tool install vsg-rs
```

## The `vsg` skill

Claude loads it when a request is about formatting VHDL, checking its style, or whether it is
ready to commit. What it carries:

* **Before committing**, the two commands, over the changed files rather than the whole tree:
  `vsg-rs --fix` then `vsg-rs --check style,lint`. Exit code 0 means nothing of error severity.
* **How to read a finding.** Anything a default run reports is a definite error, because that is
  the only class on by default. Advisory and experimental rules depend on what you meant, so they
  are read rather than obeyed. `--explain` answers what a rule id means, instead of guessing from
  the message.
* **The library map warning is not noise.** Without `vhdl_ls.toml` most of the lint layer does not
  run, and a run that says so has not given the file a clean bill of health.
* **Do not reformat by hand.** Layout the formatter would undo on its next run makes the diff
  bigger, not smaller.
* **Do not silence what you will not fix.** Rules come from the project's configuration, and an
  accepted violation is a [waiver](waivers.md), which records a reason.

## What the MCP server adds beside it

The command line is the cheaper route for files on disk: many files per run, and nothing travels
through the conversation. The [tools](mcp.md) earn their place on the case it cannot reach, which
is source that is not a file yet. An agent writing a new module can format the text and read its
findings **before** the write, so what lands on disk is already right.

## Without the plugin

The skill is only advice; every part of it works from the command line, and the MCP server can be
registered on its own:

```sh
claude mcp add vsg-rs -- vsg-rs mcp
```

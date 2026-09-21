# Architecture

```text
      VSG command line (src/vsg_cli.rs)    editors, CI
                     │                               │
                     └─────────────┬─────────────────┘
                                   ▼
                        library API (src/lib.rs)
                                   │
                 resolved configuration (src/config.rs)
                                   │
                    source snapshot: Parsed::new(bytes)
                                   │  one vhdl_syntax parse
                                   ▼
                    lossless concrete syntax tree
                 ┌─────────────────┴──────────────────┐
                 ▼                                    ▼
      layout builder (src/format.rs)          rules (diagnostics + fix intents)
                 │                                    │
                 ▼                                    ▼
      layout document (src/doc.rs)            central fix resolver
                 │                                    │
                 ▼                                    │
      width-aware printer ◄───────────────────────────┘
                 │
                 ▼
      output check (src/verify.rs): re-parse, compare tokens and comments
                 │
                 ▼
      trailing-comment alignment (src/align.rs, whitespace only)
                 │
                 ▼
            canonical source
```

## Invariants

* **One parse per snapshot.** `Parsed` owns the source bytes and the only tree built from them.
  Formatting and rules read that tree; nothing re-tokenizes the source. The output check parses
  the *output*, which is a different snapshot, and comment alignment reuses that parse. (A file
  with tool directives is parsed a second time with the directives masked.)
* **No source mutation between rules.** Rules only read the tree and return diagnostics and fix
  intents. Nothing depends on another rule having run first.
* **Formatting is one pass.** The layout builder emits every token exactly once, in source order,
  and decides only the whitespace between tokens. The printer takes each line-break decision once.
  Idempotence (`format(format(x)) == format(x)`) is a tested property, not something reached by
  repeating the formatter.
* **Output is verified.** Before any output is returned, it is re-parsed and must have no syntax
  errors and the same tokens (keywords compared case-insensitively) and comments, in the same
  order, attached to the same tokens. A mismatch is reported as an internal error and the input is
  left untouched. This is what makes format-on-save safe even when the formatter has a bug. The
  only later step, trailing-comment alignment, changes nothing but the spaces between code and
  a comment on the same line.
* **Refuse rather than guess.** Sources with syntax errors are returned unchanged with an error.
  VHDL-2019 tool directives on their own line are parsed as same-length comments (so byte
  offsets stay valid for rules and fixes) and put back into the output.
* **Deterministic.** Layout depends only on the tree and the resolved configuration. There is no
  hash-map iteration order in the output path. Files are processed in parallel worker processes,
  but results are reported in input order.
* **Atomic writes.** A file is written only after its complete result has been computed and
  verified, via a temporary file in the same directory that is then renamed over the original. Files
  whose output is identical to their input are not rewritten.

## Formatter versus linter versus fixer

* The **formatter** owns presentation: whitespace, indentation, line breaks, line folding,
  alignment and keyword case. VSG rules in those categories map onto formatter settings, not
  onto independent fixers.
* The **linter** reports rules that are not layout: naming, structure, semantics.
* The **fixer** applies the fixes VSG applies by default (and, with `--unsafe_fixes`, the rest)
  as text edits over a snapshot. A central resolver accepts non-overlapping fixes in source order; the rest are
  resolved again on the new snapshot, for a bounded number of rounds, and the result goes
  through the formatter once. There are no phases and no rule-order dependencies, and users never
  need to repeat `--fix`.

## The boundary with vhdl_ls

vsg-rs and [vhdl_ls](https://github.com/VHDL-LS/rust_hdl) share a front end and divide the work.
They are built from the same crates (`vhdl_syntax` parses for the formatter, `vhdl_lang`
resolves names for the lint layer), and that is deliberate: writing a second VHDL parser and a
second name resolver to avoid sharing one would be a larger project than this one, and the
result would agree with no other tool about what the language means.

Sharing the crates is not the same as sharing the job. The division is:

| | |
|---|---|
| **vhdl_ls** | what a name means and where it is: completion, hover, go to definition, references, rename, symbols |
| **vsg-rs** | how the source should look and what it does wrong: formatting, style rules, static analysis, fixes |

`vsg-rs lsp` advertises only the second half, and [says so in its capabilities](lsp.md): it
answers no completion, hover, definition, reference, rename or symbol request, so an editor never
asks it for one. The two are independent: neither requires the other, and installing both gives
the union rather than a conflict.

[`vsg-rs mcp`](mcp.md) is the same half again, for a reader that is a coding agent rather than an
editor. It is a façade over the same library entry points, so an agent and a person are told the
same thing about the same file.

The VS Code extension is a client of both servers. Its [editing actions](vscode-editing.md)
(instantiating an entity, declaring a port map's signals, and so on) read what they write back
from VHDL-LS and never answer a question about what a name means themselves. So the boundary
holds: those requests still go to the server that owns them, and nothing in the extension parses
VHDL to get round that.

The rule this sets is about *behaviour*, not dependencies. A capability belonging to the left
column does not move into vsg-rs because the crate that could implement it is linked already. If
vsg-rs ever answers a navigation request, that is the boundary breaking, whatever the dependency
graph says.

The cost of sharing is real and accepted: `vhdl_syntax` decides which VHDL vsg-rs can parse (see
[compatibility](compatibility.md)), and a change inside it can be a change here. That is the
trade for not maintaining a second front end.

## Crate layout

A single package (`vsg-rs`) with a library (`vsg_rs`) and a binary (`vsg-rs`, VSG's command
line). The binary has no formatting logic. Range formatting (`format_range`, `fix_range`) is a
line diff of the whole-file result, so it has the same guarantees. Declarations shared between
files (`rules::Project`) are collected in a first pass when several files are checked together.

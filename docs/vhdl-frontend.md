# VHDL frontend

vsg-rs does not have its own VHDL parser. It uses the Rust VHDL ecosystem maintained in
[VHDL-LS/rust_hdl](https://github.com/VHDL-LS/rust_hdl). This document records what was
investigated (September 2026) and why the current choice was made.

## Libraries investigated

| Crate | Version | License | Role |
|---|---|---|---|
| `vhdl_syntax` | 0.2.0 (crates.io, 2026-09-08); git `419ec78` (2026-09-16) | MPL-2.0 | Lossless concrete syntax tree (CST) parser |
| `vhdl_lang` | 0.88.0 | MPL-2.0 | AST parser, semantic analysis, and the formatter used by `vhdl_ls` |
| `vhdl_ls` | 0.88.0 | MPL-2.0 | Language server; a process, not a library we want to embed |
| `vhdl-dump-ast` | 0.1.0 | MPL-2.0 | CLI that dumps `vhdl_syntax` trees |

## `vhdl_syntax`: capabilities

* **Lossless**: every byte of the input is part of the tree. Writing the tree back reproduces
  the input exactly. Whitespace, newlines (LF, CR, CRLF, and form feeds and vertical tabs, which
  the tokenizer treats as line breaks), line comments and block comments are stored as
  *trivia*.
* **Trivia model**: each token has only *leading* trivia. The trivia between two tokens belongs to
  the second one; `SyntaxToken::trailing_trivia()` is derived from the next token.
* **Tokens**: `TokenKind` covers every VHDL delimiter, keyword, literal class (abstract,
  character, string, bit string), identifiers (including extended identifiers), VHDL-2019 tool
  directives, and an `Unknown` kind for illegal input.
* **Source ranges**: `SyntaxToken::range()` (with trivia) and `text_range()` (token text only),
  in byte offsets.
* **Tree**: rowan-style green/red tree. `SyntaxNode` has typed kinds (`NodeKind`, about 330)
  generated from an ungrammar (`xtask/doc/vhdl-08-modified.ungram`). Typed AST wrappers are
  generated too. Nodes are non-empty, so `first_token()` / `last_token()` return a token, not an
  `Option`.
* **Error recovery**: the parser always produces a tree and returns a list of `SyntaxErr`s.
  Recovered trees contain missing or extra tokens, so vsg-rs refuses to format a file with any
  syntax error.
* **Standards**: `parse_with_standard(VHDLStandard, …)`. The crate documents that standards other
  than VHDL-2008 currently change little. VHDL-2019 constructs are partially supported; tool
  directives are tokenized.
* **Rewriting**: `SyntaxNode::rewrite*` produces modified trees (upstream examples use it for
  import sorting and renaming). The formatter does not need it because it builds a layout
  document instead.
* **Helpers**: `requires_separator(t1, t2, standard)` says whether two tokens need whitespace
  between them to lex correctly (LRM §15.3). vsg-rs uses it as a safety net under its own
  spacing rules.
* **Formatting facilities**: `vhdl_syntax::fmt` only handles text encoding (Latin-1/UTF-8). It is
  not a pretty printer.

## `vhdl_lang` formatter

`vhdl_lang` contains `VHDLFormatter`, which formats design units from the `vhdl_lang` AST plus
its token stream (the one `vhdl_ls` uses for LSP formatting). It keeps comments and existing
newlines. It has no width model or group/fill layout, and it works on a different tree than
`vhdl_syntax`. Its approach, keeping the user's newlines, conflicts with the canonical,
width-driven layout vsg-rs needs. Reusing it would mean rewriting most of it, so vsg-rs has its
own layout engine on top of `vhdl_syntax` (see `line-folding.md`). If the rust_hdl project wants
a width-aware formatter later, that engine is a candidate to contribute.

## Limitations found

* Syntax errors are expected in files using PSL, some VHDL-2019 constructs, or vendor
  extensions. Across about 4,400 local real-world files (tsfpga, VUnit, nvc, the rust_hdl standard
  libraries, project code), the 2008 parser rejected about 3%, mostly nvc's VHDL-2019
  regression tests.
* Tool directives (`` `if `` and similar) are tokens without structure. vsg-rs refuses to format
  files containing them for now.
* The published 0.2.0 is missing master fixes that matter to a formatter, for example an
  infinite recursion for `postponed` statements (fixed 2026-09-16).

## Decision

* Use `vhdl_syntax` as a Cargo **library** dependency: no subprocess, no fork, no vendored files.
* Pin a git revision until a crates.io release contains the needed fixes, then switch back to a
  version requirement.
* Parse each source snapshot exactly once, with `parse_with_standard(VHDL2008, …)`.
* Add `vhdl_lang` when semantic rules need name resolution. This has happened: the lint layer
  uses it for name resolution and type checking, and it is a dependency today.

## Fallback strategy

* Files the parser rejects are reported and left untouched. There is no guessing.
* Generic parser bugs found by vsg-rs (for example a construct parsed into the wrong node) are
  reported upstream with a minimal reproducer, not worked around in a fork.
* A construct that parses correctly but has no dedicated layout rule falls back to the generic
  layout. The output check still guarantees the token stream and comments are unchanged.

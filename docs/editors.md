# Editor integration

There are two ways to use vsg-rs from an editor.

**As a language server.** `vsg-rs lsp` gives diagnostics, quick fixes and formatting; see
[language server](lsp.md). On VS Code the
[extension](https://github.com/ru551n/vsg-rs/tree/main/editors/vscode) launches it for you, and
adds [editing actions](vscode-editing.md) that work through VHDL-LS. Every other editor launches
`vsg-rs lsp` directly.

**As a formatter only**, described below, for editors that just pipe a buffer through a command.

Editors run vsg-rs as an external formatter: the buffer is piped through

```sh
vsg-rs --stdin --fix --stdin_filename path/to/file.vhd                  # whole buffer
vsg-rs --stdin --fix --stdin_filename path/to/file.vhd --range 10:24    # only lines 10 to 24
```

The buffer is read from stdin. With exit code 0, stdout is the complete new buffer (with
`--range START:END`, 1-based and inclusive, only lines in that range differ from the input);
remaining violations are listed on stderr. With exit code 1, stdout is empty and stderr explains
why (for example a syntax error); leave the buffer unchanged. `--stdin_filename` is used to find
the configuration and in messages only. Add `--unsafe_fixes` only if you review the result.

For diagnostics in an editor, use a language server such as
[vhdl_ls](https://github.com/VHDL-LS/rust_hdl) together with vsg-rs as the formatter, or run
`vsg-rs --stdin --stdin_filename <path> -of syntastic` from a generic linter integration (one
`ERROR: file(line)rule -- message` line per violation).

## VS Code

With an extension that runs external formatters (for example *Custom Local Formatters*):

```json
"customLocalFormatters.formatters": [
  {
    "command": "vsg-rs --stdin --fix --stdin_filename ${file}",
    "languages": ["vhdl"]
  }
]
```

Enable `"editor.formatOnSave": true` for format-on-save.

## Neovim

With [conform.nvim](https://github.com/stevearc/conform.nvim):

```lua
require("conform").setup({
  formatters = {
    vsg_rs = { command = "vsg-rs", args = { "--stdin", "--fix", "--stdin_filename", "$FILENAME" } },
  },
  formatters_by_ft = { vhdl = { "vsg_rs" } },
  format_on_save = { timeout_ms = 1000 },
})
```

## Helix

`languages.toml`:

```toml
[[language]]
name = "vhdl"
formatter = { command = "vsg-rs", args = ["--stdin", "--fix"] }
auto-format = true
```

## Emacs

With [apheleia](https://github.com/radian-software/apheleia):

```elisp
(with-eval-after-load 'apheleia
  (add-to-list 'apheleia-formatters
               '(vsg-rs "vsg-rs" "--stdin" "--fix" "--stdin_filename" filepath))
  (add-to-list 'apheleia-mode-alist '(vhdl-mode . vsg-rs)))
```

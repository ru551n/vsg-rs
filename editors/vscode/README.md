# vsg-rs for VS Code

Formatting, diagnostics and quick fixes for VHDL, from
[vsg-rs](https://github.com/ru551n/vsg-rs).

This extension is a launcher. It starts `vsg-rs lsp` and speaks LSP to it; it contains no VHDL
parsing, no formatter and no rules of its own. What you see in the editor is what
`vsg-rs --check style,lint` and `vsg-rs --fix` produce on the command line.

## What it gives you

* **Diagnostics** in the Problems panel, including related locations — a multiply driven signal
  points at each driver, a combinational loop at each signal on it.
* **Quick fixes** for findings that carry one, and **Fix All** for the document.
* **Format Document**, and format on save.

## It is not a VHDL language server

Completion, hover, go to definition, references and rename are not here, and this extension does
not advertise them. For those, install a VHDL language server such as
[VHDL-LS](https://marketplace.visualstudio.com/items?itemName=hbohlin.vhdl-ls) and run it
alongside; the two are independent and neither requires the other.

## Getting the server

The extension currently needs `vsg-rs` installed:

```sh
pip install vsg-rs        # or: uv tool install vsg-rs
```

By default it runs `vsg-rs` from your `PATH`. To use a particular build, see
[choosing a server](docs/server-selection.md).

## Make it the VHDL formatter

If another extension also offers formatting, name this one:

```jsonc
"[vhdl]": {
    "editor.defaultFormatter": "ru551n.vsg-rs",
    "editor.formatOnSave": true
}
```

## Configuring the rules

Not here. Rules, layout and severities come from the project's own `vsg-rs.yaml` (or `vsg.yaml`
passed on the command line), found from the file's directory upwards — the same file the command
line and CI read. That is deliberate: a formatting decision must not depend on which editor
someone opened the file in.

See [the vsg-rs documentation](https://vsg-rs.readthedocs.io/).

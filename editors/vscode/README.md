# vsg-rs for VS Code

VHDL in VS Code: formatting, diagnostics and quick fixes from
[vsg-rs](https://github.com/ru551n/vsg-rs), editing actions such as instantiating an entity, and
syntax colouring with forty-three themes.

It is two things, and only one of them needs anything else installed:

* **Lint and format** run the vsg-rs language server, which this extension ships. What you see in
  the editor is what `vsg-rs --check style,lint` and `vsg-rs --fix` produce on the command line.
  Nothing else is required.
* **Editing actions** are built on the analysis [VHDL-LS](https://github.com/VHDL-LS/rust_hdl)
  already does. They need VHDL-LS running, with a `vhdl_ls.toml` for the workspace. See
  [what the editing actions need](#what-the-editing-actions-need).

## Lint and format

* **Diagnostics** in the Problems panel, including related locations. A multiply driven signal
  points at each driver, a combinational loop at each signal on it.
* **Quick fixes** for findings that carry one, and **Fix All** for the document.
* **Format Document**, and format on save.
* **Waive a finding**, on the line, in the file or everywhere, with a reason that is written to the
  project's waiver file.
* **Sort library and use clauses** as a source action: `ieee` first, then alphabetical, `work`
  last.

## Editing actions

Each one reads what it writes back from VHDL-LS. None of them parses VHDL.

* **Instantiate Entity**, from a picker or as a completion, with the library clause it needs.
* **Declare Signals for Port Map**, with the port's type and the generic's value substituted.
* **Create State Machine from Enum Type**.
* **Add Library and Use Clause for Symbol**, also as a quick fix on an unresolved name.
* **Inlay hints** for each port's direction and type in a port or generic map.
* **Map missing ports**, **Declare Entity as Component**, **Extract to constant or signal**,
  **Remove Unused Use Clauses**, a references **CodeLens** on entities, and **signature help** in
  a port map.
* **VHDL Design**, a tree of every entity, through its architectures to the instances they hold.

All of it is described, with examples and limits, in
[editing VHDL in VS Code](https://vsg-rs.readthedocs.io/en/latest/vscode-editing/).

## Colour

The extension contributes the `vhdl` language definition, a TextMate grammar and forty-three themes:
**Gruvbox VHDL Dark** and **Gruvbox VHDL Light**, and forty-one dark ones named for the schemes
they take their colours from, such as **Tokyo Night VHDL Night**, **Catppuccin VHDL Mocha** and
**Kanagawa VHDL Wave**. The grammar covers what VHDL-LS never
classifies (keywords, comments, strings, literals, operators); every name is coloured from its
semantic token, so a constant, a generic and an enumeration literal look different because the
analyser says they are, not because of how they are spelled.

## What the editing actions need

**VHDL-LS, running, with a `vhdl_ls.toml` for the workspace.** Install the
[VHDL-LS extension](https://marketplace.visualstudio.com/items?itemName=hbohlin.vhdl-ls) or run
`vhdl_ls` yourself. Without it:

* the editing commands report that no language server answered;
* names stay uncoloured, because the grammar leaves every identifier to VHDL-LS;
* **lint and format keep working**, since they come from the vsg-rs server.

The extension does not depend on VHDL-LS being installed, so it never installs one for you. And
`work` is not a valid library name in `vhdl_ls.toml`: the server ignores it without a word.

## vsg-rs is not a VHDL language server

Hover, go to definition, references and rename are VHDL-LS's, and the vsg-rs server does not
advertise them. The two are independent and neither requires the other; installing both gives you
the union. The editing actions above ask VHDL-LS what a name means and never answer that
themselves.

## For a coding agent, not an editor

The same binary serves the same answers over the Model Context Protocol, which is what an agent
writing VHDL in this workspace should be told:

```sh
claude mcp add vsg-rs -- vsg-rs mcp
```

```json
{ "mcpServers": { "vsg-rs": { "command": "vsg-rs", "args": ["mcp"] } } }
```

It offers `lint`, `format` and `explain_rule`, all on a buffer rather than a path, so a mistake
can be caught before the file is written. See
[the MCP server](https://vsg-rs.readthedocs.io/en/latest/mcp/).

## Getting the server

Nothing to install: the extension ships the server for your platform and uses it. To run a
different one, a system install or a local build, see
[choosing a server](docs/server-selection.md).

## Make it the VHDL formatter

Name this extension for VHDL, which also settles it if another VHDL extension offers formatting
too:

```jsonc
"[vhdl]": {
    "editor.defaultFormatter": "ru551n.vsg-rs",
    "editor.formatOnSave": true
}
```

Formatting goes through the same entry point as `vsg-rs --fix`, so saving applies the safe rule
fixes as well as the layout, and a file that does not parse is left alone. See
[formatting and fixing](docs/formatting.md) for that, for quick fixes and Fix All, for running
alongside VHDL-LS, and for troubleshooting.

## Configuring the rules

Not here. Rules, layout and severities come from the project's own `vsg-rs.yaml` (or `vsg.yaml`
passed on the command line), found from the file's directory upwards: the same file the command
line and CI read. That is deliberate: a formatting decision must not depend on which editor
someone opened the file in.

See [the vsg-rs documentation](https://vsg-rs.readthedocs.io/).

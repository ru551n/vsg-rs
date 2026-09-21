# Formatting and fixing

What the extension changes in a file, when it changes it, and how to tell VS Code to use it.

## Make vsg-rs the VHDL formatter

The extension identifier is **`ru551n.vsg-rs`** (publisher `ru551n`, extension name `vsg-rs`).
Name it for VHDL files, in user or workspace `settings.json`:

```jsonc
"[vhdl]": {
    "editor.defaultFormatter": "ru551n.vsg-rs",
    "editor.formatOnSave": true
}
```

Set it inside the `[vhdl]` block rather than globally, so it decides VHDL only and leaves the
formatter of every other language alone.

The same thing without editing JSON: open a `.vhd` file, run **Format Document With…** from the
command palette, then **Configure Default Formatter…**, and choose vsg-rs.

### When another VHDL extension also formats

Several VHDL extensions register a formatter for the `vhdl` language, and VS Code will not guess
between them. With no `editor.defaultFormatter` for `[vhdl]`, one of two things happens, and
neither is what you want:

* **Format Document** shows a notification that there are multiple formatters for VHDL and asks
  you to configure a default. Formatting does not happen until you do.
* On save, that notification is easy to miss, or another extension's formatter is used because it
  was configured as the default earlier. The file is saved either unformatted, or formatted by
  something else.

Setting `editor.defaultFormatter` as above settles it. The other extension can stay installed;
only its formatter stops being used for VHDL.

## Format on save

Formatting goes through the same entry point as `vsg-rs --fix`, not through a separate
editor-only formatter. Saving therefore applies **both**:

* every **safe rule fix**: each fix a rule carries, apart from the unsafe ones below; the same
  set `vsg-rs --fix` applies on the command line;
* the **layout**, which vsg-rs decides for the whole file at once: whitespace, indentation, line
  structure, folding and alignment. See
  [formatting](https://vsg-rs.readthedocs.io/en/latest/formatting/).

This is deliberate: layout alone would leave the safe fixes unapplied, and CI running `vsg-rs`
would then disagree with what the editor just wrote. After a save, `vsg-rs --fix` has nothing
left to change in that file.

What it never does:

* **Unsafe fixes.** The fixes `--unsafe_fixes` exists for may change what the design does, and
  are never applied on save, offered as a quick fix, or included in fix-all.
* **Touch a file that does not parse.** A file with a syntax error gets its syntax errors as
  diagnostics and no edits at all: the buffer is left exactly as you wrote it.

The rules and the layout come from the project's own `vsg-rs.yaml`, found from the file's
directory upwards. There is no VS Code setting for any of it; see the
[extension README](../README.md#configuring-the-rules).

## Quick fixes and Fix All

A finding that carries a fix offers it as a **Quick Fix**: the lightbulb on the line, `Ctrl+.`
with the cursor on the finding, or the quick-fix entry on the row in the **Problems** panel. The
edit is what `--fix` would do to that one violation.

**Fix all vsg-rs findings** is a source action over the whole document, equal to what `--fix`
would write to the file. It appears in the code-actions menu, and can be run on save:

```jsonc
"[vhdl]": {
    "editor.codeActionsOnSave": { "source.fixAll": "explicit" }
}
```

Only fixes vsg-rs would apply itself are offered, in either form; an `--unsafe_fixes` fix is
never among them.

Fix-all and format-on-save produce the same file, so enabling both is harmless but redundant:
pick whichever you prefer. `"source.fixAll"` without a language block also runs the fix-all
action of every other extension that provides one.

## Running alongside VHDL-LS

vsg-rs is not a VHDL language server and does not advertise itself as one. It deliberately does
not offer completion, hover, go to definition, declaration, type definition, implementation,
references, rename, document symbols, workspace symbols, semantic tokens, signature help or
inlay hints, so VS Code never asks it for an answer it has no business giving.

| | |
|---|---|
| [VHDL-LS](https://marketplace.visualstudio.com/items?itemName=hbohlin.vhdl-ls) | completion, hover, go to definition, references, rename, symbols, and what the [editing actions](https://vsg-rs.readthedocs.io/en/latest/vscode-editing/) are built on |
| vsg-rs | diagnostics with related locations, quick fixes, formatting |

The two are independent, neither requires the other, and installing both gives you the union.
To keep VHDL-LS for language intelligence and have vsg-rs format:

1. Install both extensions.
2. Set `editor.defaultFormatter` for `[vhdl]` to `ru551n.vsg-rs`, as above. VHDL-LS keeps
   answering everything else.

Diagnostics from both appear in the Problems panel; each row shows the source that produced it,
and vsg-rs's rows carry the rule id as the code. They share the project's `vhdl_ls.toml`: it is
the same library map, so nothing extra is needed for vsg-rs's cross-file rules once VHDL-LS is
set up.

For the full division of labour, see
[language server](https://vsg-rs.readthedocs.io/en/latest/lsp/).

## Troubleshooting

**The output channel** is where the server's own messages go: **vsg-rs: Show Output** from the
command palette. For the LSP traffic as well, set `"vsg-rs.trace.server": "messages"` (or
`"verbose"`). **vsg-rs: Restart Server** restarts it without reloading the window.

**The server will not start.** The extension says so and names the executable it tried.
**vsg-rs: Show Server Version** prints which one that was, what the extension is, and what the
server reports itself to be. The extension ships a server for the common platforms and uses it by
default, so this usually means either the platform has no bundled build (install vsg-rs and set
`vsg-rs.server.mode` to `systemPath`) or `vsg-rs.server.path` points at something that is not
there. See [choosing a server](server-selection.md).

**No diagnostics at all.** Check that VS Code recognises the file as VHDL; the extension
activates on the `vhdl` language, for `.vhd` and `.vhdl`. If the file has a syntax error, the
syntax errors are all you get: no rule runs on a tree that does not represent the source.

**Style findings but few lint findings.** The usual cause is a missing `vhdl_ls.toml`. Most of
the lint rules have to resolve names across files, which needs the library map; without one they
are skipped, exactly as on the command line. See
[project setup](https://vsg-rs.readthedocs.io/en/latest/project-setup/). vsg-rs looks for the
file in the server's working directory, so open the folder that holds it rather than a
subdirectory or a single file.

**Saving changes nothing.** Either another extension is still the default formatter for VHDL
(see above), or the file does not parse, or it is already what `--fix` would write.

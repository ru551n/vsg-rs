# Working on the extension

The extension lives in the vsg-rs repository, at `editors/vscode`, next to the other ways vsg-rs
is distributed (`python/` for the wheel, `action.yml` for the GitHub Action). It is packaged from
the same release that produces the server binaries, so the two cannot drift apart.

```sh
cd editors/vscode
npm install
npm run compile      # or: npm run watch
```

Then open `editors/vscode` in VS Code and run the **Run Extension** launch configuration, which
opens a second window with the extension loaded.

Point it at your own build rather than a released one:

```jsonc
"vsg-rs.server.mode": "userPath",
"vsg-rs.server.path": "${workspaceFolder}/target/debug/vsg-rs"
```

## What belongs here, and what does not

This is a launcher and nothing else. VHDL parsing, formatting, rules, fixes and configuration all
live in the Rust crate: if a behaviour needs changing, it changes there, so the command line and
the editor change together.

Adding language intelligence — completion, hover, navigation — is out of scope. Those belong to a
VHDL language server; see `docs/lsp.md` in the repository root.

## Checks

`npm run compile` type-checks; CI runs it for every change.

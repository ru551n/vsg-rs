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

There are two halves, and they follow different rules.

**Lint and format** are a launcher. VHDL parsing, formatting, rules, fixes and configuration all
live in the Rust crate: if a behaviour needs changing, it changes there, so the command line and
the editor change together. `src/extension.ts` starts the server and nothing more.

**The editing actions** (`src/editing.ts`, with the pure generators in `src/generate.ts`) are
client code, because they are edits an editor performs on the answers of a language server. The
constraint on them is different and just as firm: **no VHDL is parsed here.** Everything about a
design comes back from VHDL-LS through VS Code's provider commands, and the generators only
transform what it said. A regular expression that reads an entity's ports is a second VHDL parser
that will disagree with the server, so add the missing provider call instead.

Language intelligence itself, hover, navigation and rename, is out of scope for vsg-rs and stays
with VHDL-LS; see `docs/lsp.md` in the repository root. Keep `src/generate.ts` free of VS Code
imports so it stays checkable without an editor.

## Checks

`npm run compile` type-checks, and CI runs it for every change. The tsconfig is strict, including
unused locals and parameters.

`npm test` runs the checks that need nothing but Node 22.18 or newer: the generators, against hover
text taken verbatim from a real server, and the grammar, by tokenizing a sample and asserting the
scopes. CI runs it too.

`npm run smoke` is the one that matters for `src/editing.ts`, and it is not in CI. Those commands
only mean something against a running language server, so it builds a small two-library project,
starts VS Code with the extension and a real `vhdl_ls`, and runs the commands, answering their
prompts:

```sh
npm run compile
VHDL_LS=/path/to/vhdl_ls npm run smoke
```

It needs a display, the `code` command, and the VHDL-LS extension installed in that VS Code. The
`vhdl_ls` must be able to find its `vhdl_libraries`; a build installed with cargo alone panics
without them. Run it after any change to `src/editing.ts`.

Two things it learned the hard way, worth knowing before changing anything it covers:

* a `vhdl_ls.toml` that names a library `work` is silently ignored by the server, so a test
  project must use another name;
* a query for workspace symbols returns at most 200, counting every port and architecture, so
  listing a project's entities has to go through each file's document symbols.

`scripts/probe.mjs` shows what VHDL-LS answers for a project, which is what to look at first when
a generator misreads something.

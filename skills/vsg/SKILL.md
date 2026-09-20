---
name: vsg
description: Use when formatting or style-checking VHDL, or checking VHDL is clean before committing: laying out a file the project's way, fixing style violations, running the linter over changed VHDL, or answering what a rule id means. Typical requests include "format this VHDL", "lint this", "check the style", "is this ready to commit", "what does entity_019 mean", "clean up the formatting".
---

# VHDL formatting and static analysis

`vsg-rs` formats VHDL and reports what is wrong with it. One binary, two layers: **style** (the
VHDL Style Guide's rule set, plus a formatter that fixes what it reports) and **lint** (rules
that resolve names, so they catch what a per-file checker cannot).

Check it is there before promising anything:

```sh
vsg-rs --version    # not installed: `pip install vsg-rs` or `uv tool install vsg-rs`
```

Never reformat VHDL by hand while this is available. Hand-formatting a file the tool would lay
out differently makes the next run's diff bigger, not smaller.

## Before committing

The common case, and it is two commands over the changed files rather than the tree:

```sh
files=$(git diff --name-only --diff-filter=ACM HEAD -- '*.vhd' '*.vhdl')
vsg-rs --fix $files                    # format, and apply every safe fix
vsg-rs --check style,lint $files       # what is left, from both layers
```

Exit code 0 means nothing of error severity was reported, 1 means something was (or an input was
missing, or an argument was invalid). Warnings alone do not fail a run.

Read the second command's output before committing. `--fix` has already dealt with everything
mechanical, so whatever remains needs a decision: the code changes, or the finding is wrong.

## Reading what it reports

Every lint rule carries a class, and the class is the instruction:

| Class | What it means |
|---|---|
| **definite error** | cannot work, or does not mean what it says. Fix it. The only class on by default |
| **advisory** | depends on what you meant. Worth reading, not automatically worth changing |
| **experimental**, **policy** | on only when the project asked for them |

So anything a default run reports is a definite error. `vsg-rs --explain <rule>` gives a rule's
description, class and default state; use it rather than inferring meaning from a rule id.

## The lint layer needs a library map

The rules that resolve names across files (`lint_001` to `lint_502`) need to know which library
each file is in, which comes from the project's `vhdl_ls.toml`. Without one they do not run, and
the run says so:

```
WARNING: no vhdl_ls.toml found, so 54 of 78 lint rules did not run.
```

**Report that warning if it appears.** A clean run that skipped most of the layer is not a clean
file, and treating it as one is exactly what the warning exists to prevent. Keep the map in the
project: one in `$HOME` scoops up every VHDL file on the machine.

## Commands worth knowing

| Command | What it does |
|---|---|
| `vsg-rs --fix FILE...` | format and apply every safe fix, in place |
| `vsg-rs --fix --diff FILE...` | the same as a unified diff, changing nothing |
| `vsg-rs --check style,lint FILE...` | report both layers |
| `vsg-rs lint FILE...` | the lint layer alone |
| `vsg-rs --recursive src` | every `.vhd` and `.vhdl` under a directory |
| `vsg-rs --explain RULE` | what one rule means |
| `vsg-rs --list_rules` | every rule, its layer, its class, and whether it is on |

`--unsafe_fixes` also applies fixes that may change behaviour or lose information. Do not pass it
routinely; when it is genuinely wanted, review the diff first.

## The MCP tools, for source that is not a file yet

This plugin registers the `vsg-rs` MCP server, which answers the same things through tools. One
of them does something the command line cannot:

- `lint` and `format` take **either a path or a buffer**. A buffer is source that has not been
  written, so VHDL can be checked *before* it reaches the disk. Prefer this when generating a new
  module: format the text, read the findings, then write the file already correct.
- `format` with `write: true` fixes a file in place and returns only what changed, instead of the
  whole file.
- `explain_rule` is `--explain`.

For files already on disk with a shell available, the command line is still cheaper: it takes
many files in one run and nothing travels through the conversation.

## The configuration is the project's

Rules, severities and layout come from the project's own configuration file, found by walking up
from the file being checked. Do not pass `-c` to override it, and do not disable a rule to
silence a finding you would rather not fix. If a violation is genuinely accepted, that is what
waivers are for (`--generate_waivers`, then `--waivers`), and they record a reason.

## Do not

- Report a formatting or lint result no run produced. Run the command and quote it.
- Reformat by hand, or "fix" layout that the formatter would undo on its next run.
- Run over the whole tree when a few files changed: it buries the findings that are yours.
- Treat a run that warned about a missing library map as a clean bill of health.

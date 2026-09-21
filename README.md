# vsg-rs

[![CI](https://github.com/ru551n/vsg-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/ru551n/vsg-rs/actions/workflows/ci.yml)
[![Documentation](https://readthedocs.org/projects/vsg-rs/badge/?version=latest)](https://vsg-rs.readthedocs.io/)
[![PyPI](https://img.shields.io/pypi/v/vsg-rs)](https://pypi.org/project/vsg-rs/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#licence)

**Catch it earlier. Earlier than simulation.**

A VHDL formatter and linter in Rust. Every bug has a price that goes up the longer it takes to
find: a moment in your editor, a coffee in CI, an afternoon in a waveform viewer, a respin on
silicon. vsg-rs moves what it can to the cheap end of that scale: the moment you save the file.

Some of what it reports a simulator would have told you eventually: an index outside its array,
a value outside its subtype, a process that can never suspend. *Eventually* means after you have
written the testbench, elaborated the design and waited for the run, and only if the run reaches
that line. Some of it no simulator will ever tell you, because the design elaborates perfectly
well and simply does not mean what it says.

It runs the rule set, command line and configuration of the
[VHDL Style Guide](https://vhdl-style-guide.readthedocs.io/) (VSG), adds a formatter that fixes
what it reports, and adds a lint layer that VSG has no equivalent of.

A default lint run reports **definite errors only**: things that cannot work, rather than things
worth a look. Everything that depends on what you meant is one line of configuration away.

**Status: beta.** The style layer implements VSG 3.35's rule set and is tested against more than
11,000 real-world files; expect layout changes before 1.0. The lint layer (`vsg-rs lint`) is
newer and its rule set is still growing.

## Install

```sh
pip install vsg-rs            # Linux, Windows and macOS wheels, Python 3.10+
uv tool install vsg-rs        # or
cargo install --path .        # from source (Rust 1.95 or newer)
```

Standalone binaries for Linux (static, x86_64 and aarch64), Windows (x64 and arm64) and macOS
(arm64 and x86_64) are attached to each
[release](https://github.com/ru551n/vsg-rs/releases), with a `SHA256SUMS` file.

## Everything VSG accepts, unchanged

```sh
vsg-rs -f src/*.vhd -c vsg.yaml --fix
```

The arguments, the configuration file, the reports and the exit codes are VSG's, and
[VSG documents them](https://vhdl-style-guide.readthedocs.io/en/latest/usage.html). Existing
scripts and CI jobs keep working, so this README covers only what vsg-rs adds on top.
Differences are listed in
[compatibility](https://vsg-rs.readthedocs.io/en/latest/compatibility/).

## What vsg-rs adds

**A real formatter.** Source is parsed once into a lossless syntax tree and printed in one
canonical layout, like rustfmt or Black. Every layout rule is fixed rather than reported, long
lines are folded at structural boundaries, and running `--fix` twice changes nothing. All
violations are reported at once and fixed in a single pass: no repeated runs, no rule-order
dependencies.

**Safety.** Formatted output is re-parsed and must contain exactly the same tokens and comments;
files with syntax errors are never changed. Fixes VSG does not apply by default need
`--unsafe_fixes`.

**Speed.** Real-world VHDL is checked at about 3.4 MB/s on one core, in parallel worker
processes.

### Beyond style: `vsg-rs lint`

Rules that need names resolved, which a per-file style checker cannot do:

```sh
vsg-rs lint --recursive src                  # the lint layer
vsg-rs --recursive src --check style,lint    # both in one run
```

```
lint_600 -- Signal 'flag' is not assigned on every path of this combinational process,
            which infers a latch
lint_601 -- Signal 'result' is assigned by 2 concurrent statements (lines 34, 38)
lint_001 -- The signal 'b' is not read in the sensitivity list (first read at line 10)
```

Sensitivity lists, unused declarations, latch inference, multiple drivers, combinational loops,
state machines, vector widths, clock-domain crossings, and the type and name diagnostics of a
real front end. Every rule states the evidence behind it and reports nothing when that evidence
is missing.

Most of these resolve names across files, which needs a library map. Without one they are
skipped, and the run says so. See
[static analysis](https://vsg-rs.readthedocs.io/en/latest/lint/) and
[project setup](https://vsg-rs.readthedocs.io/en/latest/project-setup/).

### Adopting a rule set on existing code

```sh
vsg-rs --recursive src --generate_waivers waivers.yaml   # accept what exists today
vsg-rs --recursive src --waivers waivers.yaml            # from now on, only new violations
```

[Waivers](https://vsg-rs.readthedocs.io/en/latest/waivers/) record a rule, a file glob, lines and
a reason, and never affect the exit code.

### Options vsg-rs adds

| Option | Meaning |
|---|---|
| `lint`, `--check style,lint` | run the lint layer, or both layers |
| `--lint_configuration FILE` (`-lc`) | configuration for the lint layer only |
| `--waivers`, `--generate_waivers`, `--show_waived` | accept known violations |
| `--unsafe_fixes` | also apply fixes VSG does not apply by default; review the result |
| `--diff` | with `--fix`, print a unified diff instead of changing files |
| `--range START:END` | with `--fix`, change only these lines |
| `--stdin_filename PATH` | name of the `--stdin` input, for configuration lookup and reports |
| `--sarif FILE` | SARIF 2.1.0, for GitHub code scanning |
| `--sonarqube FILE` | SonarQube generic issue JSON |
| `--recursive` | check the VHDL files in directories and their subdirectories |
| `--list_rules`, `--statistics` | what each rule is, and how often each fired |

Configuration keys vsg-rs adds live under a `vsg_rs:` block: `reflow_comments`,
`testbench_files`, `testbench_libraries`, `synchronizers`, and a `rule:` block per kind of
file.

### Editors and coding agents

Two servers over stdio, both the same engine as the command line, so everything is told the same
thing about the same file under the same configuration.

```sh
vsg-rs lsp                        # a language server: diagnostics, formatting, quick fixes
vsg-rs mcp                        # an MCP server, for a coding agent
```

The [language server](https://vsg-rs.readthedocs.io/en/latest/lsp/) is meant to run beside
`vhdl_ls` rather than instead of it, and advertises only what vsg-rs is: it answers no
completion, hover or definition request. A [VS Code extension](editors/vscode/README.md) ships
it.

The [MCP server](https://vsg-rs.readthedocs.io/en/latest/mcp/) gives a coding agent three tools,
`lint`, `format` and `explain_rule`:

```sh
claude mcp add vsg-rs -- vsg-rs mcp
```

```json
{ "mcpServers": { "vsg-rs": { "command": "vsg-rs", "args": ["mcp"] } } }
```

`lint` and `format` take a file path or a buffer. A buffer is how an agent checks what it is
about to write before writing it, which is one step earlier still; `format` with `write` fixes a
file in place without moving it through the conversation.

For Claude Code there is a plugin, which installs a `vsg` skill and registers the MCP server:

```text
/plugin marketplace add ru551n/vsg-rs
/plugin install vsg-rs@vsg-rs
```

The skill tells an agent to format and check VHDL before committing it, how to read a finding's
class, and not to treat a run that skipped the library map as a clean file. `vsg-rs` itself still
has to be on `PATH`.

### CI

A [GitHub Action](https://vsg-rs.readthedocs.io/en/latest/github-action/) posts annotations and
suggested changes; [GitLab CI](https://vsg-rs.readthedocs.io/en/latest/gitlab-ci/) gets the
code-quality report, and SonarQube the generic issue JSON. Jenkins reads the SARIF file through
Warnings-NG. Every [report format](https://vsg-rs.readthedocs.io/en/latest/reports/) works
anywhere.

```yaml
- uses: ru551n/vsg-rs@v0.11.0
  with:
    args: --recursive src
```

## Documentation

**[vsg-rs.readthedocs.io](https://vsg-rs.readthedocs.io/)**: what vsg-rs adds on top of VSG. The
rules, their options and the configuration file are VSG's own and are linked to rather than
repeated.

* [Quick start](https://vsg-rs.readthedocs.io/en/latest/quick-start/)
* [Static analysis](https://vsg-rs.readthedocs.io/en/latest/lint/) and
  [project setup](https://vsg-rs.readthedocs.io/en/latest/project-setup/)
* [Rule reference](https://vsg-rs.readthedocs.io/en/latest/rule-reference/)
* [Running in an airgap](https://vsg-rs.readthedocs.io/en/latest/airgapped/): what it needs, what
  it cannot reach, and how to check both yourself
* [Language server](https://vsg-rs.readthedocs.io/en/latest/lsp/),
  [MCP server](https://vsg-rs.readthedocs.io/en/latest/mcp/) and
  [Claude Code plugin](https://vsg-rs.readthedocs.io/en/latest/claude-code/)
* [Waivers](https://vsg-rs.readthedocs.io/en/latest/waivers/)
* [Migrating from VSG](https://vsg-rs.readthedocs.io/en/latest/migrating-from-vsg/)
* [Compatibility with VSG](https://vsg-rs.readthedocs.io/en/latest/compatibility/), measured
  weekly against two corpora

## Disclosure: this code was written by an LLM

Most of this repository was written by Claude, directed and reviewed by a human. Treat that as a
reason to check it rather than a reason to trust it, so here is what there is to check against.
None of it depends on the code having been written well.

**It cannot reach anything.** No network, proven four ways in CI on every change: no dependency
is an HTTP or TLS client, the binary imports no symbol that can reach a host, everything runs
inside an empty network namespace, and `strace` records zero network syscalls. You can run those
same checks against the binary you downloaded in about five minutes:
[running in an airgap](https://vsg-rs.readthedocs.io/en/latest/airgapped/).

**It cannot quietly mangle your files.** Formatted output is re-parsed and must contain exactly
the same tokens and comments, in the same order, before anything is written. A mismatch is an
internal error and your file is left untouched, so a formatter bug costs you a run rather than a
file. Files with syntax errors are never modified, writes are atomic through a temporary file in
the same directory, and a file whose output equals its input is not rewritten at all.

**Its blast radius is small.** `#![forbid(unsafe_code)]`. No privileges, no service, no daemon,
no telemetry. The only state it keeps is a 2.3 MB cache of the embedded `ieee` and `std` sources,
written once per version. The only time it starts another program is `--local_rules`, which runs
the VSG on your `PATH`, because only VSG can run VSG's Python plugins.

**It is checked against reality, not against itself.** The formatter is compared with VSG's own
output over a corpus of real VHDL, weekly. Every lint rule must report nothing on three real
projects unless a person has confirmed each finding is a genuine defect, and those counts are a
CI gate. Fuzzing runs nightly. Agreement is
[published per rule](https://vsg-rs.readthedocs.io/en/latest/compatibility/) rather than claimed.

**Known limits, stated rather than buried.** A dependency (`vhdl_lang`) can stack-overflow on
pathological input, which aborts the process before anything is written. A file you made
read-only is still replaced, because atomic writes need permission on the directory rather than
on the file. Both are in the
[airgap page](https://vsg-rs.readthedocs.io/en/latest/airgapped/).

Cautious first run? `--fix --diff` changes nothing and prints what it would do, `--backup` keeps
a copy beside each file, and a run without `--fix` never writes anything at all.

## Relationship to VSG

> **vsg-rs is an independent Rust implementation of a VHDL formatter and style checker that aims
> for compatibility with the rules and configuration of the VHDL Style Guide (VSG). It is not
> affiliated with, endorsed by, or maintained by the VHDL Style Guide project or its
> maintainers.**
>
> vsg-rs was inspired by the
> [VHDL Style Guide (VSG)](https://github.com/jeremiah-c-leary/vhdl-style-guide) project by
> Jeremiah Leary and contributors. vsg-rs contains no VSG code; VSG is used only as a behavioural
> reference.

Every release states the VSG version it targets, and `vsg-rs --version` prints it.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the build, test and corpus commands, how compatibility
is measured, and what CI runs.

## Licence

Either [Apache License, Version 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
Third-party dependencies are listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) and in
[NOTICE](NOTICE); the VHDL parser, `vhdl_syntax` from the rust_hdl project, is MPL-2.0 and is
used as an unmodified dependency.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
vsg-rs, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

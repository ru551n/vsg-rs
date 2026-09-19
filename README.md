# vsg-rs

[![CI](https://github.com/ru551n/vsg-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/ru551n/vsg-rs/actions/workflows/ci.yml)
[![Documentation](https://readthedocs.org/projects/vsg-rs/badge/?version=latest)](https://vsg-rs.readthedocs.io/)
[![PyPI](https://img.shields.io/pypi/v/vsg-rs)](https://pypi.org/project/vsg-rs/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#licence)

A VHDL formatter and linter in Rust. It runs the rule set, command line and configuration of the
[VHDL Style Guide](https://vhdl-style-guide.readthedocs.io/) (VSG), adds a formatter that fixes
what it reports, and adds a lint layer that VSG has no equivalent of.

**Status: beta.** The style layer implements VSG 3.35's rule set and is tested against more than
11,000 real-world files; expect layout changes before 1.0. The lint layer (`vsg-rs lint`) is
newer: 58 rules today, and the set will grow.

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
violations are reported at once and fixed in a single pass — no repeated runs, no rule-order
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

Sensitivity lists, unused declarations, latch inference, multiple drivers, register naming, and
the type and name diagnostics of a real front end. Testbench and RTL code can carry different
rules, and `ieee`/`std` are built in, so nothing else needs installing. See
[the lint layer](https://vsg-rs.readthedocs.io/en/latest/lint/).

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
| `--recursive` | check the VHDL files in directories and their subdirectories |
| `--list_rules`, `--statistics` | what each rule is, and how often each fired |

Configuration keys vsg-rs adds live under a `vsg_rs:` block: `reflow_comments`,
`testbench_files`, `testbench_libraries`, and a `rule:` block per kind of file.

### CI

A [GitHub Action](https://vsg-rs.readthedocs.io/en/latest/github-action/) posts annotations and
suggested changes; [GitLab CI](https://vsg-rs.readthedocs.io/en/latest/gitlab-ci/) gets the
code-quality report. SARIF, JUnit and `--statistics` work anywhere.

```yaml
- uses: ru551n/vsg-rs@v0.11.0
  with:
    args: --recursive src
```

## Documentation

**[vsg-rs.readthedocs.io](https://vsg-rs.readthedocs.io/)** — what vsg-rs adds on top of VSG. The
rules, their options and the configuration file are VSG's own and are linked to rather than
repeated.

* [The lint layer](https://vsg-rs.readthedocs.io/en/latest/lint/)
* [Waivers](https://vsg-rs.readthedocs.io/en/latest/waivers/)
* [Migrating from VSG](https://vsg-rs.readthedocs.io/en/latest/migrating-from-vsg/)
* [Compatibility with VSG](https://vsg-rs.readthedocs.io/en/latest/compatibility/), measured
  weekly against two corpora

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

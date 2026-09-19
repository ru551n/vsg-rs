# vsg-rs

A fast Rust-native VHDL formatter and style checker with the command line, rules and
configuration of VSG.

> **vsg-rs is an independent Rust implementation of a VHDL formatter and style checker that
> aims for compatibility with the rules and configuration of the VHDL Style Guide (VSG). It is not
> affiliated with, endorsed by, or maintained by the VHDL Style Guide project or its
> maintainers.**
>
> vsg-rs was inspired by the [VHDL Style Guide (VSG)](https://github.com/jeremiah-c-leary/vhdl-style-guide)
> project by Jeremiah Leary and contributors. vsg-rs contains no VSG code; VSG is used only as a
> behavioural reference.

**Status: beta.** vsg-rs is tested against more than 11,000 real-world files. 192 VSG rules are
implemented as rules with fixes (structure, identifier case and consistency, naming, comments,
`length_001`). The other 779 rules are layout rules covered by the formatter: whitespace,
indentation (including `indent.tokens`), blank lines, alignment, keyword case and line
structure. Expect layout changes before 1.0.

## Why

* **Drop-in for VSG.** Same arguments, same configuration files, same report formats and exit
  codes. Existing scripts and CI jobs keep working.
* **One phase.** All violations are reported at once, and `--fix` fixes everything in one run:
  no repeated `--fix` runs, no rule-order dependencies.
* **A real formatter.** Source is parsed once into a lossless syntax tree and printed in one
  canonical layout, like rustfmt or Black. Running `--fix` twice changes nothing.
* **Long lines are folded, not just reported.** Calls, maps, aggregates, expressions, conditions,
  declarations and signatures fold at structural boundaries
  ([line folding](docs/line-folding.md)).
* **Safe to run on save.** Formatted output is re-parsed and must contain exactly the same tokens
  and comments; fixed output must parse. Files with syntax errors are never changed. Fixes that
  VSG does not apply by default, or that could break code, need `--unsafe_fixes`.
* **Fast.** A typical file takes a few milliseconds; real-world VHDL is checked at about
  3.4 MB/s on one core, and files are processed in parallel worker processes
  ([performance](docs/performance.md)).

## Installation

```sh
pip install vsg-rs            # Linux, Windows and macOS wheels, Python 3.10+ (or: uv tool install vsg-rs)
cargo install --path .        # from source (Rust 1.95 or newer)
```

Standalone binaries for Linux (static, x86_64 and aarch64), Windows (x64 and arm64) and macOS
(arm64 and x86_64) are attached to each [GitHub release](https://github.com/ru551n/vsg-rs/releases),
with a `SHA256SUMS` file.

## Usage

`vsg-rs` takes VSG's arguments:

```sh
vsg-rs -f src/fifo.vhd src/fifo_pkg.vhd      # report violations
vsg-rs src/*.vhd                             # file names can also be given without -f
vsg-rs -f src/*.vhd --fix                    # fix and format the files in place
vsg-rs -f src/*.vhd --fix -b                 # ... keeping a .bak copy of each changed file
vsg-rs -f src/*.vhd -c vsg.yaml              # with a VSG configuration file
vsg-rs -f src/*.vhd -of summary -js report.json -j junit.xml --quality_report gl.json
vsg-rs --stdin < src/fifo.vhd                # read from stdin
vsg-rs -rc entity_015                        # the configuration of one rule
vsg-rs -oc effective.json                    # the whole effective configuration
vsg-rs --style indent_only -f src/*.vhd --fix  # only re-indent
vsg-rs --recursive src                       # every .vhd/.vhdl file below src
```

As in VSG, files can also be listed in the configuration (`file_list`), and directories are
not searched unless `--recursive` is given. The exit code is `0` when no error-severity violations
were found and `1` otherwise.

### What is reported

* **Rule violations**, with VSG's rule ids and solution texts, in VSG's console layout
  (`-of vsg`, the default), as `-of syntastic` lines or as an `-of summary`.
* **Layout violations**: every place where `--fix` would change the layout (indentation,
  spacing, line breaks, blank lines, trailing whitespace, keyword case, comment columns), under
  the VSG rule that reports it. vsg-rs decides the whole layout at once; the rule for each kind
  of change was learned by comparing with VSG on real code. Changes without a known VSG rule are
  reported as `format`.

When several files are checked together, uses of names declared in another file's package, or
of another file's entity ports and generics, are checked for consistent capitalization too.

### Fixes

`--fix` applies the fixes VSG applies by default and then formats the file. Rules that VSG
does not fix by default (for example adding a missing port mode, `port_023`, or removing a
signal's default value, `signal_007`) are left alone unless their configuration sets
`fixable: true` or `--unsafe_fixes` is given. `--fix_only FILE` limits fixing to the listed
rules and lines, as in VSG.

### Additions to VSG's command line

| Option | Meaning |
|---|---|
| `--unsafe_fixes` | with `--fix`, also apply fixes VSG does not apply by default; they may change behaviour or remove information, so review the result |
| `--diff` | with `--fix`, print a unified diff instead of changing files |
| `--stdin_filename PATH` | name of the `--stdin` input, used to find the configuration and in reports |
| `--range START:END` | with `--fix`, change only these lines (1-based), from stdin or a file |
| `--sarif FILE` | write a SARIF 2.1.0 report (GitHub code scanning) |
| `--list_rules` | list every VSG rule and how vsg-rs handles it |
| `--statistics` | print the violations per rule over all inputs, most first |
| `--recursive` | check the `.vhd` / `.vhdl` files in directories given as inputs, and in their subdirectories |

With `--stdin --fix`, the fixed source is written to stdout and the report to stderr (VSG 3.35
cannot fix stdin). `-fp` and `-ap` are accepted and have no effect, `--force_fix` has no effect
(files with syntax errors are never changed), and local rules (`-lr`, VSG's Python rule plugins)
are run by an installed VSG (see below). See [compatibility](docs/compatibility.md) for the details.

### Configuration

Configuration files use VSG's format (YAML or JSON) and are passed with `-c`; later files
override earlier ones. Without `-c`, vsg-rs uses the nearest `vsg-rs.yaml` / `.vsg-rs.yaml`
(or `.json`) next to the first input or in a parent directory, which VSG itself ignores.

```yaml
rule:
  global:
    indent_style: spaces
    indent_size: 2
  group:
    case::name:
      case: lower
  length_001:
    length: 100
  process_016:
    disable: true
  port_023:
    fixable: true          # also add missing port modes with --fix
indent:
  tokens:
    case_statement_alternative:
      when_keyword: {after: current, token: current}
file_rules:
  legacy/**/*.vhd:
    rule:
      length_001:
        disable: true
```

Local rules (`-lr DIR` or `local_rules: DIR`) are VSG Python plugins, so vsg-rs runs them with
an installed VSG (`vsg` on the path, or the command in `VSG_RS_VSG`, for example
`uvx --from vsg==3.35.0 vsg`) with all built-in rules disabled, and merges their findings;
with `--fix`, their fixes are applied first.

Blank-line, alignment, keyword-case and indentation rules configure the formatter
([formatting](docs/formatting.md)). Formatting can be switched off for a region with
`-- vsg-rs: fmt off` / `-- vsg-rs: fmt on`; VSG's `-- vsg_off [rule ...]` / `-- vsg_on` comments
suppress rules (and, without rule names, formatting).

### Editor integration

Configure your editor to pipe the buffer through
`vsg-rs --stdin --fix --stdin_filename <path>` (add `--range START:END` to format selected
lines). With exit code 0 the buffer is replaced with stdout; otherwise stdout is empty, stderr
explains why, and the buffer should be left unchanged. See [editor integration](docs/editors.md)
for VS Code, Neovim, Helix and Emacs.

### GitHub Action

vsg-rs is also a GitHub Action. Add a workflow such as `.github/workflows/vhdl-style.yml`:

```yaml
name: VHDL style
on:
  push:
    branches: [main]
  pull_request:

jobs:
  vsg:
    runs-on: ubuntu-latest
    permissions:
      contents: write          # only to resolve suggestions that no longer apply (else: read)
      pull-requests: write     # suggestions and the summary comment
    steps:
      - uses: actions/checkout@v5
      - uses: ru551n/vsg-rs@v0.11.0
        with:
          args: -c vsg.yaml --recursive src   # any vsg-rs arguments
```

The action downloads the vsg-rs release of its own tag (checked against `SHA256SUMS`) and
runs `vsg-rs` with `args`. On a pull request:

* **Suggested changes**: what `--fix` would change on the pull request's lines is posted as
  suggestions in one review, applied with one click. Suggestions that no longer apply are
  resolved on the next run.
* **One summary comment**: findings per rule and the command that fixes them, updated in place
  on every push.
* **Annotations** on the lines the pull request adds or changes.
* **Code scanning** (optional, `sarif-upload: true` with `security-events: write`): rule
  violations also become tracked alerts under *Security → Code scanning*, and GitHub's
  code scanning bot comments on the ones a pull request introduces.
* **Result**: the step fails when vsg-rs reports error-severity violations
  (`fail-on-violations: false` only reports them).

Other inputs: `version` (a release tag or `latest`), `working-directory`, `annotations`
(`changed`, `true` or `false`), `layout` (`suggestions`, or `alerts` for one code scanning
alert per block to reformat), `pr-comment`, and `token`. Outputs: `exit-code`, `sarif-file`
and `version`. Linux, Windows and macOS runners are supported. To fix the reported violations
locally, run the same arguments with `--fix`. See [GitHub Action](docs/github-action.md) for
details, and [ru551n/vhdl-ai-test#9](https://github.com/ru551n/vhdl-ai-test/pull/9) for an
example pull request.

### Python

The wheels install the `vsg-rs` executable. `python -m vsg_rs ...` runs it too, and
`vsg_rs.find_vsg_rs_bin()` returns its path.

## Documentation

Published at **[vsg-rs.readthedocs.io](https://vsg-rs.readthedocs.io/)**. It documents what
vsg-rs adds on top of VSG; the rules, their options and the configuration file are VSG's own and
are linked to rather than repeated.

* [Compatibility with VSG](docs/compatibility.md) and [rule status](docs/rule-status.md)
* [Formatting](docs/formatting.md) (layout, alignment, blank lines, keyword case, indentation),
  [line folding](docs/line-folding.md) and its [coverage matrix](docs/line-folding-coverage.md)
* [Migrating from VSG](docs/migrating-from-vsg.md) (including pre-commit and CI)
* [GitHub Action](docs/github-action.md) and code scanning, [GitLab CI](docs/gitlab-ci.md)
<<<<<<< HEAD
* [The lint layer](docs/lint.md): `vsg-rs lint`, rules that need names resolved
=======
* [Waivers](docs/waivers.md): accepting known violations so only new ones are reported
>>>>>>> origin/main
* [Editor integration](docs/editors.md)
* [vsg-rs next to Linty and Sigasi](docs/comparison.md): what a per-file style linter cannot do,
  and the [roadmap to a full linter](docs/roadmap-linter.md)
* [Architecture](docs/architecture.md) and the [VHDL frontend](docs/vhdl-frontend.md)
  (why `vhdl_syntax`)
* [Performance](docs/performance.md)
* [Releasing](docs/releasing.md) (Python package, binaries, platforms, release workflow)
* [VSG configuration model](docs/vsg-config.md), [VSG rule catalog](docs/vsg-rules.md)
* [Known VSG bugs](docs/upstream-bugs.md) and [limitations](docs/upstream-limitations.md) that
  vsg-rs is designed to avoid

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo run --release --example corpus -- --width 80 path/to/vhdl   # stability and overflow report
FIX=1 cargo run --release --example corpus -- path/to/vhdl         # the same for --fix (FIX=unsafe: --unsafe_fixes; CONFIG=file)
cargo run --release --example bench                                # timing on generated inputs
python scripts/compare_vsg.py FILE...                              # findings per rule, vsg-rs vs VSG 3.35
python scripts/learn_layout_rules.py FILE...                       # relearn src/layout_rules.json
python scripts/gen_spacing_rules.py VSG_CHECKOUT/docs              # regenerate src/spacing_rules.json
UPDATE_EXPECT=1 cargo test --test golden                           # re-bless golden files (review the diff)
```

CI also runs `cargo deny check` (licences, duplicate crates, advisories; `deny.toml`),
`cargo machete` (unused dependencies), `typos` (`_typos.toml`), `cargo test --doc`,
`cargo llvm-cov` (coverage in the job summary), `cargo semver-checks` against the base branch
(reported, not enforced) and `examples/bench` against the base branch
(`scripts/compare_bench.py`, warns when something is more than 25% slower). The formatter is
fuzzed nightly (`cargo +nightly fuzz run format`, `fuzz/fuzz_targets/format.rs`).

The library (`vsg_rs`) can also be used directly: `Parsed::new`, `format_parsed`, `fix_with`,
`rules::check_with` and the range variants `format_range` / `fix_range`.

## License

vsg-rs is licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option. Third-party dependencies are listed in
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md). The VHDL parser, `vhdl_syntax`
from the rust_hdl project, is MPL-2.0 and is used as an unmodified dependency.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
vsg-rs, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

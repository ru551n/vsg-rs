# vsg-rs

**Catch it earlier. Earlier than simulation.**

A VHDL formatter and static-analysis tool written in Rust, with the rule set, command line and
configuration of the [VHDL Style Guide](https://vhdl-style-guide.readthedocs.io/) (VSG).

A bug costs more the longer it takes to find: a moment in your editor, a coffee in CI, an
afternoon in a waveform viewer, a respin on silicon. Some of what vsg-rs reports a simulator
would have told you eventually — an index outside its array, a value outside its subtype, a
process that can never suspend — but only once the testbench exists, the design elaborates and
the run reaches that line. Some of it no simulator will ever tell you, because the design works
and simply does not mean what it says.

Source is parsed once into a lossless syntax tree and printed in one canonical layout, so
formatting is decided rather than negotiated. On top of that sits a lint layer that resolves names
across files and reports what a per-file style checker cannot see. VSG compatibility means an
existing project keeps its configuration and its CI; the formatter and the lint layer are why you
would choose vsg-rs on a project that has never used VSG.

## What it does

**Formats.** One canonical layout, applied rather than reported. Long lines are folded at
structural boundaries, output is re-parsed and must contain exactly the same tokens and comments,
and running `--fix` twice changes nothing. Suitable for format-on-save.

**Analyses.** Sensitivity lists, unused declarations, name resolution, case-choice completeness,
port and generic association, latch inference, multiple drivers, combinational loops, state
machines, vector widths and clock-domain crossings. Each rule states the evidence it works from,
and reports nothing when the evidence is missing.

**Stays compatible with VSG.** The arguments, the configuration file, the reports and the exit
codes are VSG's, so existing scripts keep working.

## Try it

```sh
pip install vsg-rs

vsg-rs -f src/top.vhd                      # report style violations
vsg-rs --recursive src --fix               # format in place
vsg-rs lint --recursive src                # the lint layer
vsg-rs --recursive src --check style,lint  # both
```

## Where to go next

| If you want to | Go to |
|---|---|
| install it and run something | [Quick start](quick-start.md) |
| move an existing VSG project across | [Coming from VSG](migrating-from-vsg.md) |
| format code, or format on save | [Formatting](formatting.md), [Editors](editors.md) |
| find real bugs, not layout | [Static analysis](lint.md) |
| understand why a rule reported nothing | [Project setup](project-setup.md) |
| look up what `lint_600` means | [Native rules](native-rules.md), [rule reference](rule-reference.md) |
| run it in CI | [GitHub](github-action.md), [GitLab](gitlab-ci.md), [Report formats](reports.md) |
| accept violations in existing code | [Waivers](waivers.md) |
| know what an option does | [CLI reference](cli.md) |
| know how it works inside | [Architecture](architecture.md) |

## What it is not

vsg-rs is not a compiler, simulator, synthesis tool, timing analyser or formal verification tool,
and it does not elaborate a design. The lint layer reasons about source: names, types, control
flow, dataflow and the connections between design units. Where the source does not say something
outright, it generally prefers to report nothing over guessing — see
[what it does not try to infer](lint.md#what-it-does-not-try-to-infer).

## Status

Beta. The style layer implements VSG 3.35's rule set and is tested against more than 11,000
real-world files; layout may still change before 1.0. The lint layer is newer and its rule set is
growing. Each release states the VSG version it targets, and `vsg-rs --version` prints it.

vsg-rs is an independent implementation. It contains no VSG code, and is not affiliated with or
endorsed by the VHDL Style Guide project.

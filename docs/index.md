# vsg-rs

A VHDL formatter and linter in Rust. It runs [VSG](https://vhdl-style-guide.readthedocs.io/)'s
rule set with VSG's command line and VSG's configuration, and adds a formatter that fixes what it
reports, a lint layer that needs names resolved, and the output formats CI actually wants.

```sh
pip install vsg-rs          # or a standalone binary from the releases
vsg-rs --recursive src      # exactly as VSG would
vsg-rs --recursive src --fix
vsg-rs lint --recursive src # the layer VSG does not have
```

## This documentation does not repeat VSG's

The rules, their options and the configuration file are VSG's, and VSG documents them well.
Repeating that here would only let the two drift apart, so **the VSG interface is documented by
VSG**:

| For | Read |
|---|---|
| What a rule checks and its options | [VSG rule documentation](https://vhdl-style-guide.readthedocs.io/en/latest/rule_groups.html) |
| The configuration file: `rule:`, `indent:`, `file_list` | [Configuring VSG](https://vhdl-style-guide.readthedocs.io/en/latest/configuring.html) |
| The command line arguments vsg-rs shares | [Using VSG](https://vhdl-style-guide.readthedocs.io/en/latest/usage.html) |

What follows is only what vsg-rs adds on top, and where it deliberately differs.

## What vsg-rs adds

* **[The lint layer](lint.md)** — `vsg-rs lint`: sensitivity lists, unused declarations, latch
  inference, multiple drivers, register naming, and the type and name diagnostics of a real
  front end. None of this exists in VSG.
* **[Waivers](waivers.md)** — accept the violations a project has decided to live with, so a
  rule set can be adopted on code that does not follow it yet.
* **[Formatting](formatting.md)** — nearly every layout rule is fixed rather than reported, with
  [line folding](line-folding.md) for long lines and formatter-off regions.
* **CI** — a [GitHub Action](github-action.md) with annotations and suggested changes,
  [GitLab CI](gitlab-ci.md) with the code-quality report, plus SARIF, JUnit and `--statistics`.
* **[Editor integration](editors.md)**, and a [migration guide](migrating-from-vsg.md) for
  projects coming from VSG.

## How close it is to VSG

Measured, not asserted: [compatibility](compatibility.md) has the weekly comparison against VSG
over two corpora, and [rule status](rule-status.md) lists what is implemented.

Every release states the VSG version it targets, and `vsg-rs --version` prints it.

## Background

* [vsg-rs next to Linty and Sigasi](comparison.md), and the
  [roadmap to a full linter](roadmap-linter.md)
* [Architecture](architecture.md) and the [VHDL frontend](vhdl-frontend.md)
* [Performance](performance.md)

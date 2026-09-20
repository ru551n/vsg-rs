# Static analysis

The root command is VSG's: style rules, applied per file, byte-identical reports. `vsg-rs lint`
runs the other layer.

**A default run reports definite errors.** Every finding follows from the source and the
resolved project: the program cannot do what it says, whatever anyone intended by it. Nothing is
reported because it looks unusual, because it is probably a mistake, or because some tool
downstream would refuse it.

That is a promise about your attention. A finding is something to correct rather than something
to weigh up, so an empty report means vsg-rs found nothing it can prove, and a report with one
line in it means it found something real.

Rules that answer broader questions — whether a declaration is used, whether a state can be
reached, whether a name follows the house convention — are still here, and still exact about the
facts they state. They are off unless asked for, because what to conclude from the fact depends
on what you meant rather than on what the language requires. See
[asking for more](#asking-for-more).

```sh
vsg-rs --recursive src                          # style
vsg-rs lint --recursive src                     # static analysis
vsg-rs --recursive src --check style,lint       # both in one run
```

`lint` is a subcommand only when it is the first argument and no file of that name exists, so a
directory called `lint` still wins and every VSG command line keeps working. Underneath it is
`--check`, which takes the layers as a comma-separated list.

## What it works from

```text
source
  │
  ├─ lossless syntax tree ──────────────┐
  │                                     │
  └─ VHDL front end                     │
       │                                │
       ├─ resolved names and types      │
       └─ design-unit relationships     │
                     │                  │
                     ▼                  ▼
            resolved-semantic      dataflow and
                 rules            structural rules
                     └────────┬───────────┘
                              ▼
                         diagnostics
```

Nothing here elaborates the design. There is no netlist, no constant propagation across generics,
no simulation. What the analyser has is the syntax tree, a resolved symbol table, and the port
connections between design units.

## The principle

**vsg-rs reports what it can derive. Where the source does not say something outright, it
generally reports nothing rather than guessing.**

That is a deliberate trade. A tool that guesses produces findings that are individually plausible
and collectively worthless, because nobody can tell which ones to read. Every rule here was
measured against three real code bases — an FPGA accelerator, VUnit and open-logic — and had to
report **zero** findings on all of them before being added, unless a finding was confirmed real.
A rule that cannot reach zero is either not shipped or narrowed until it can.

The consequence is that vsg-rs under-reports. `lint_740` compares vector widths only when both
sides are whole objects with literal ranges, so most parameterised RTL is out of its reach.
`lint_700` accepts a single-stage synchroniser although two stages are the usual requirement. In
both cases the alternative was a rule that fires on correct code.

### How certain is a finding

Every rule carries a class, and the class decides whether a default run uses it. The
[rule reference](rule-reference.md) is grouped by it, `--list_rules` prints it beside each rule,
and the configuration layer reads the same answer, so the three cannot disagree.

| Class | What a finding means | Default |
|---|---|---|
| **Definite error** | the program cannot do what it says | **on** |
| **Advisory** | an exact fact whose significance depends on what you meant | off |
| **Experimental** | inferred design intent, not derived from the source | off |
| **Policy** | a convention the language has no opinion about | off |

The line between the first two is what the source has to say about it. That an enumeration value
is never assigned is a fact; that it is a *mistake* is a guess about the design, so `lint_710` is
advisory. That a configuration names an architecture nothing declares is also a fact, and there
is nothing to guess about what it means: elaboration fails. That one is definite.

## Asking for more

A whole class at a time:

```yaml
rule:
  group:
    advisory:
      disable: false
```

or one rule, which works the same as it always did:

```yaml
rule:
  lint_712:
    disable: false
```

An enabled rule reports at `error` severity like any other, so it fails a build the same way.
If you want it reported without failing anything, set `severity: warning` on the rule or the
group.

## What it does not try to infer

* **Whether a signal is a clock or a reset**, except where a process tests its edge.
* **What a generic will be**, so a width or range depending on one is not evaluated.
* **What an instance does internally** when its entity is not in the analysed set: everything it
  touches is treated as driven, so nothing is reported about it.
* **Whether two drivers are deliberate.** Two drivers on a resolved type such as `std_logic` are
  legal VHDL -- that is what a resolution function is for -- so `lint_601` is advisory and off
  unless asked for. A narrower rule for drivers the language does not permit needs the resolved
  type, which the analyser does not have yet.
* **Anything requiring elaboration**: resource use, timing, reachable state.

## Rules

The [rule reference](rule-reference.md) lists every rule with what it reports, grouped by how
sure it is. `vsg-rs --explain lint_740` prints the same for one rule, and `--list_rules` prints
all of them alongside the style rules, each with its class and whether a default run uses it.

!!! important "Most of them need a library map"

    Resolving a name means knowing which library each file belongs to. Without a
    `vhdl_ls.toml` most of the layer does not run, and the run says how much. See
    [Project setup](project-setup.md).

## What the structural checks assume

These assumptions are why a finding means what it says:

* **A process is combinational** unless it tests a clock edge (`rising_edge`, `falling_edge`,
  `'event`) or suspends on `wait`. Testbench processes therefore never infer latches.
* **A latch needs a path with no assignment**: an `if` without `else`, or a `case` alternative
  that skips a signal the others assign. A signal assigned unconditionally first has a default
  and is not reported, which is the usual way to write combinational logic.
* **`q(3)` and `q(4)` are one signal** for the multiple-driver rule, so a vector driven bit by bit
  from two processes is reported.
* **A register belongs to the clock its process is edge-triggered on**, and a crossing captured
  into a flop and nothing else is a synchroniser's first stage.

## Which layers fail the build

Every finding carries its layer, derived from the rule id so it cannot disagree with what produced
it. `--fail_on` chooses which layers make the run fail; the rest are still reported:

```sh
vsg-rs --recursive src --check style,lint --fail_on lint   # lint gates CI, style only informs
```

`--fix` belongs to the style layer and is refused without it: a lint finding never carries a fix,
because applying one would change what the design does.

## Unsaved buffers

The layer analyses source, not files. `--stdin` with `--stdin_filename` analyses a buffer in the
context of the project its path belongs to — see
[unsaved files](project-setup.md#unsaved-files).

## Configuration

`-c` takes several files and merges them in order. `--lint_configuration` (`-lc`) adds files that
apply to the lint layer only, so an analysis policy can sit beside a style policy without touching
it:

```sh
vsg-rs --recursive src --check style,lint -c vsg.yaml -lc lint.yaml
```

Rules can also be configured per kind of file — see
[telling RTL from testbenches](project-setup.md#telling-rtl-from-testbenches).

## Adopting it on existing code

The first `--check lint` run on an existing project will fail the build. That is the point, and
[waivers](waivers.md) are the release valve: `--generate_waivers` writes a file accepting what
exists today, so only new findings are reported from then on.

## Next

* [Project setup](project-setup.md) — the library map, and which rules need it
* [Rule reference](rule-reference.md) — every rule and its evidence
* [Native rules in detail](native-rules.md) — worked examples for the ten vsg-rs implements
* [CLI reference](cli.md) — every option, generated from the binary
* [Waivers](waivers.md) — accepting known findings
* [Report formats](reports.md) — getting findings into CI

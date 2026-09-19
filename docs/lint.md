# Static analysis

The root command is VSG's: style rules, applied per file, byte-identical reports. `vsg-rs lint`
runs the other layer — code that parses and is legal, but is probably not what anyone meant.

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

Rules differ in the evidence behind them, and the [rule reference](rule-reference.md) groups them
by it:

| Group | Evidence | What a finding means |
|---|---|---|
| Resolved semantics | a resolved symbol table: a name, its declaration, its type | a fact about the code |
| Dataflow and structure | the syntax tree plus port connections | proven from structure |
| Heuristic | inferred design intent | weaker; written to under-report |
| Style policy | a naming convention you configured | not analysis at all |

Only two rules are heuristic — `lint_600` (latch inference) and `lint_700` (clock-domain
crossings) — because both must decide what a process *is* before they can say anything. They are
labelled as such rather than mixed in with the rest.

## What it does not try to infer

* **Whether a signal is a clock or a reset**, except where a process tests its edge.
* **What a generic will be**, so a width or range depending on one is not evaluated.
* **What an instance does internally** when its entity is not in the analysed set: everything it
  touches is treated as driven, so nothing is reported about it.
* **Whether two drivers are deliberate.** `lint_601` reports them even for resolved types such as
  `std_logic`, where a tri-state is legal, because far more often it is a mistake. Waive the
  deliberate ones.
* **Anything requiring elaboration**: resource use, timing, reachable state.

## Rules

The [rule reference](rule-reference.md) lists every rule with what it reports, grouped by
evidence. `vsg-rs --explain lint_600` prints the same for one rule, and `--list_rules` prints all
of them alongside the style rules.

!!! important "Most of them need a library map"

    Resolving a name means knowing which library each file belongs to. Without a `vhdl_ls.toml`
    only 13 rules run, and the run says so. See [Project setup](project-setup.md).

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

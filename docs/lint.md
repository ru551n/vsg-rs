# The lint layer

vsg-rs has two layers of rules, chosen with `--check`:

| Layer | What it checks | Default |
|---|---|---|
| `style` | VSG's ~972 rules and the formatter. Syntactic, per file. | on |
| `lint` | Code that is legal but probably wrong: sensitivity lists, unused declarations, latches, multiple drivers, types. Needs names resolved. | off |

```sh
vsg-rs --recursive src                       # style only, byte-identical to VSG
vsg-rs --recursive src --check style,lint    # both
vsg-rs --recursive src --check lint          # lint only
```

`--fix` belongs to `style` and is refused without it: a lint finding never carries a fix, because
fixing one would change what the design does.

Lint rules are **errors unless you say otherwise**. A linter that only warns is a linter nobody
reads; turn individual rules down in the configuration instead:

```yaml
rule:
  lint_004:
    severity: warning      # unused declarations are advice here
  lint_601:
    disable: true          # this design drives a bus from several places on purpose
```

## Telling it where your libraries are

Everything past the first few rules needs to resolve names across files, and that needs a library
map. vsg-rs reads `vhdl_ls.toml` from the working directory — the same file VHDL-LS uses, so many
projects already have one:

```toml
[libraries]
work.files = ['src/**/*.vhd']
osvvm.files = ['osvvm/**/*.vhd']
```

**Without that file, only `lint_001` to `lint_003`, `lint_600` and `lint_601` are reported**, and
the rest are held back with a count. That is deliberate: with no map every name from another
library is unresolved, and the rules that depend on resolution then produce nonsense. On 50 VUnit
files the difference is 9968 unresolved-name findings and 623 knock-on argument errors, against
none that were real.

`ieee` and `std` are built in: they ship inside the binary and need no configuration.

## The rules

**Per file — always reported**

| Rule | |
|---|---|
| `lint_001` | A signal read by a combinational process is missing from its sensitivity list |
| `lint_002` | A signal in a sensitivity list is not read by the process |
| `lint_003` | An item that may not appear in a sensitivity list |
| `lint_600` | A combinational process does not assign a signal on every path: a latch |
| `lint_601` | A signal is assigned by more than one concurrent statement |

**With a library map** — `lint_004` (unused declarations), `lint_005` and `lint_006` (a needless
`work` library, an unused context), `lint_1xx` (names and declarations: unresolved, duplicate,
circular dependency), `lint_2xx` (types and expressions: type and dimension mismatches,
constraints, attributes), `lint_3xx` (subprograms and calls: ambiguous calls, arguments,
signatures, returns), `lint_4xx` (ports, generics and associations), `lint_5xx` (exit, next and
loop labels).

`vsg-rs --list_rules` prints all of them with a description.

## What the checks assume

* **A process is combinational** unless it tests a clock edge (`rising_edge`, `falling_edge`,
  `'event`) or suspends on `wait`. Testbench processes therefore never infer latches.
* **A latch needs a path with no assignment**: an `if` without `else`, or a `case` alternative
  that skips a signal the others assign. A signal assigned unconditionally first has a default
  and is not reported, which is the usual way to write combinational logic.
* **`q(3)` and `q(4)` are one signal** for the multiple-driver rule, so a vector driven bit by bit
  from two processes is reported. That is normally what you want to know; where a design does it
  deliberately, waive it.
* **Two drivers are legal** for a resolved type such as `std_logic`, which is how tri-states are
  written. `lint_601` reports them anyway, because far more often it is a mistake.

## Adopting it

The first `--check lint` run on an existing project will fail the build. That is the point, and
`docs/waivers.md` is the release valve: `--generate_waivers` writes a file accepting what exists
today, so only new findings are reported from then on.

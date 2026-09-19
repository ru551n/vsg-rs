# The lint layer

vsg-rs has two layers of rules. The root command is VSG's — same arguments, same reports, style
rules only — and `vsg-rs lint` runs the other one.

| Layer | What it checks | How to run it |
|---|---|---|
| style | VSG's ~972 rules and the formatter. Syntactic, per file. | `vsg-rs ...` (the root command) |
| lint | Code that is legal but probably wrong: sensitivity lists, unused declarations, latches, multiple drivers, types. Needs names resolved. | `vsg-rs lint ...` |

```sh
vsg-rs --recursive src                          # style, byte-identical to VSG
vsg-rs --recursive src --fix                    # and fix what is fixable
vsg-rs lint --recursive src                     # the lint layer
vsg-rs lint --recursive src --check style,lint  # both in one run
```

`lint` is a subcommand only when it is the first argument and no file of that name exists, so a
file called `lint` still wins and every VSG command line keeps working unchanged. Underneath it
is `--check`, which takes the layers as a comma-separated list.

Every finding carries the layer it came from. `--statistics` shows it per rule, and `--fail_on`
decides which layers make the run fail — the others are still reported:

```sh
vsg-rs --recursive src --check style,lint --fail_on lint   # lint gates CI, style only informs
vsg-rs --explain lint_600                                  # what one rule checks, and its layer
```

`--fix` belongs to `style` and is refused without it: a lint finding never carries a fix, because
fixing one would change what the design does.

## Configuration files

`-c` takes several files and merges them in order, so a repository's own configuration can sit on
top of a shared one:

```sh
vsg-rs --recursive src -c company.yaml project.yaml
```

The lint layer can have files of its own, merged after `-c` and applied only to it, so a team's
lint policy need not be mixed into its style policy:

```sh
vsg-rs lint --recursive src -c style.yaml -lc lint.yaml
vsg-rs lint --recursive src -lc company-lint.yaml project-lint.yaml
```

`-lc` is short for `--lint_configuration`. Files given there are ignored when only the style
layer runs.

Lint rules are **errors unless you say otherwise**. A linter that only warns is a linter nobody
reads; turn individual rules down in the configuration instead:

```yaml
rule:
  lint_004:
    severity: warning      # unused declarations are advice here
  lint_601:
    disable: true          # this design drives a bus from several places on purpose
```

## Testbenches get their own rules

A latch or a second driver means nothing in a testbench: it drives a signal from two places on
purpose and holds values between `wait`s by design. So each file is classified as `rtl` or
`testbench`, and each kind can carry its own rule block:

```yaml
vsg_rs:
  testbench_files: ['test/**', 'tb_*.vhd', '**/sim/*.vhd']

  testbench:
    rule:
      lint_600: {disable: true}    # latches are not a testbench problem
      lint_601: {disable: true}    # nor is driving a signal from two places
      lint_004: {severity: warning}

  rtl:
    rule:
      lint_600: {disable: false}
```

Nothing is disabled unless you say so: the blocks are yours, and without them both kinds are
checked identically.

**Or name the libraries.** A project whose `vhdl_ls.toml` already separates design from
verification does not need patterns at all:

```toml
# vhdl_ls.toml
[libraries]
rtl_lib.files = ['design/**/*.vhd']
tb_lib.files  = ['verif/**/*.vhd']
```

```yaml
# vsg.yaml
vsg_rs:
  testbench_libraries: ['tb_lib', 'osvvm']
```

Every file in those libraries is a testbench, whatever it is called and wherever it lives. This
is the one classification that needs a `vhdl_ls.toml`; the others do not.

**Patterns** are globs. One without a `/` is about the file name wherever it lives (`tb_*.vhd`),
one with a `/` is about the path and also matches deeper, so `test/**` covers
`modules/fifo/test/tb_fifo.vhd`. `*` stops at a directory separator, `**` does not.

**Without `testbench_files` or `testbench_libraries`** a file is classified by its own shape, in
this order: a
`-- vsg-rs: testbench` comment near the top, a verification library in the code (`vunit_lib`,
`osvvm`, `uvvm_util`, `runner_cfg`), an entity with no ports, then the usual names and
directories (`tb_`, `_tb`, `test/`, `sim/`, `bench/`, ...). Over three corpora this classified
0 of 18 real RTL files as testbench and 16 of 16 testbenches correctly.

`--debug` prints every file treated as a testbench and which of those signals matched, so the
classification is never silent.

### State machines

`lint_710` and `lint_711` read a state machine out of the source the way a synthesis tool
recognises one: an enumerated type, a signal of it registered under a clock, and a `case` on that
signal deciding the next state. Both one- and two-process styles work, because every signal of
the state type is considered together.

The machine has to be readable with certainty or it is left alone: every assignment to the state
must be a plain value of the type, since a state computed by a function cannot be reasoned about
from syntax. A `case` that never assigns the state is a multiplexer selecting by state, not
transition logic, and is not checked for exits.

### Wiring

`lint_730` needs to know what an instance does to the signals connected to it, so the run first
reads every input for its entities and port modes. A port map is then read as "these signals are
driven, those are read". Nothing is assumed: an instance of an entity the run cannot see marks
everything it touches as driven, a procedure call marks every name it mentions as driven (a
procedure can have `out` parameters and vsg-rs does not resolve signatures), and a signal with an
initial value is treated as tied on purpose rather than undriven.

## Telling it where your libraries are

Everything past the first few rules needs to resolve names across files, and that needs a library
map. vsg-rs reads `vhdl_ls.toml` from the working directory — the same file VHDL-LS uses, so many
projects already have one:

```toml
[libraries]
work.files = ['src/**/*.vhd']
osvvm.files = ['osvvm/**/*.vhd']
```

**Without that file, only `lint_001` to `lint_003`, `lint_600` and `lint_601` are reported.** A
run without one says so every time, whether or not it found anything:

```
WARNING: no vhdl_ls.toml found, so 53 of 58 lint rules did not run. They need to know
         which library each file is in; see docs/lint.md
```

A clean report from five rules must not be mistaken for a clean report from all of them. That is deliberate: with no map every name from another
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
| `lint_710` | A state of an enumerated state machine is never entered |
| `lint_711` | A state of an enumerated state machine has no exit |
| `lint_730` | A signal is read but nothing drives it: no assignment, and no instance output |

### Naming registers (off by default)

A signal a clocked process assigns becomes a register, and many projects want to see that in its
name. Two rules, each off until you set it:

```yaml
rule:
  lint_602:                                  # the suffix
    disable: false
    suffixes: ['_q', '_reg', 're:_p[0-9]+']  # `_p1`, `_p2`, ... without listing them
  lint_603:                                  # the prefix
    disable: false
    prefixes: ['r_']
```

Enabling either without a list of its own means the usual convention for it: `_q`, `_r` or
`_reg` for the suffix, `r_` for the prefix. Entries are plain text, or a glob (`_p?` accepts one
character after `_p`, `_p*` any number), or `re:` followed by a regular expression when the glob
is too blunt. Only signals assigned under a clock edge are checked, so combinational signals and
testbench code are never named at.

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

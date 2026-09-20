# Project setup

Most of the lint layer resolves names across files: to know that a signal is never read, the
analyser has to find every place the name could be read, which means knowing what each file
declares and which library it belongs to. That information is not in the source, so vsg-rs has to
be told.

!!! warning "Without a library map, most rules do not run"

    Of the lint layer's rules, only 13 report without one — the three sensitivity-list rules and
    the ten [native rules](rule-reference.md), which need no resolution. The rest are skipped.

Every run without a map says so, whether or not it found anything:

```
WARNING: no vhdl_ls.toml found, so 53 of 66 lint rules did not run. They need to know
         which library each file is in; see .../project-setup/
```

The run prints the counts for the version you have; [rule counts](rule-counts.md) lists them.

A clean report from a handful of rules must not be mistaken for a clean report from all of
them.

## The library map

vsg-rs reads `vhdl_ls.toml` from the working directory — the same file
[VHDL-LS](https://github.com/VHDL-LS/rust_hdl) uses, so a project with editor support usually has
one already:

```toml
[libraries]
work.files = ['src/**/*.vhd']
osvvm.files = ['osvvm/**/*.vhd']
```

Each entry names a VHDL library and the files compiled into it. Globs are relative to the file's
own directory. vsg-rs only reads this file; it never writes one.

`ieee` and `std` are built in — they ship inside the binary, so nothing needs installing and no
entry is needed for them.

## Why not guess

With no map, every name from another library is unresolved, and rules that depend on resolution
report nonsense rather than nothing. On 50 VUnit files the difference was 9,968 unresolved-name
findings and 623 knock-on argument errors, none of them real. Skipping those rules and saying so
is the behaviour that keeps the report trustworthy.

## What each file can see

Analysis covers the files the map lists, not only the files named on the command line. That is
what makes cross-file rules possible: `vsg-rs lint src/one.vhd` can still report that a name in
`one.vhd` refers to a declaration in `two.vhd`.

Files outside the map are analysed on their own. They get only the rules that need no resolution,
and are reported as unanalysed rather than silently passing.

## Telling RTL from testbenches

A testbench is written to different rules than hardware: an unused signal in a stimulus process
is normal, a latch in one is not interesting. vsg-rs classifies files and applies a separate rule
block to each kind.

Classification is explicit first. In `vsg.yaml`:

```yaml
vsg_rs:
  testbench_files:
    - 'tb_*.vhd'
    - 'test/**'
  testbench_libraries:
    - osvvm_tb
  testbench:
    rule:
      lint_004:
        disable: true
  rtl:
    rule:
      lint_600:
        severity: error
```

`testbench_files` takes glob patterns; `testbench_libraries` names libraries from `vhdl_ls.toml`.

!!! note "Automatic classification is a heuristic"

    With neither setting, vsg-rs still guesses from the source — an entity with no ports, or one
    instantiating a design under test, is treated as a testbench. It is right on the corpora it
    was measured against, but it is inference, and it changes which rules apply.

    `--debug` prints every file classified this way and why. Set `testbench_files` or
    `testbench_libraries` to decide it yourself rather than relying on the guess.

## Unsaved files

`--stdin` analyses the bytes it is given, not whatever is on disk under that name:

```sh
vsg-rs --stdin --stdin_filename src/top.vhd --check lint < buffer.vhd
```

`--stdin_filename` is what decides the file's library and what findings are labelled with, so an
editor buffer is analysed in its project even before it is saved. An edit that exists only in the
buffer changes the findings; the file itself is never read or written.

## Checking what ran

```sh
vsg-rs lint --recursive src --debug
```

`--debug` reports the files treated as testbenches. `--statistics` reports how often each rule
fired, which is the quickest way to see that a rule you expected is reporting nothing because it
never ran.

## Next

* [Static analysis](lint.md) — what the layer reports and how certain it is
* [Rule reference](rule-reference.md) — every rule, grouped by the evidence behind it
* [Waivers](waivers.md) — accepting what exists today

# Contributing

Thanks for looking. Bug reports with a small VHDL reproducer are the most useful thing you can
send: formatting is compared byte for byte, so a file and the output you expected is usually
enough to act on.

## Building and testing

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## The corpus and the benchmarks

Most real bugs here are found by running over real code rather than by unit tests, so there are
harnesses for it:

```sh
cargo run --release --example corpus -- --width 80 path/to/vhdl   # stability and overflow report
FIX=1 cargo run --release --example corpus -- path/to/vhdl        # the same for --fix
                                                                  # (FIX=unsafe: --unsafe_fixes; CONFIG=file)
cargo run --release --example bench                               # timing on generated inputs
UPDATE_EXPECT=1 cargo test --test golden                          # re-bless golden files, then read the diff
```

## Comparing against VSG

vsg-rs's compatibility is measured, not asserted:

```sh
python scripts/compare_vsg.py FILE...                  # findings per rule, vsg-rs vs VSG
python scripts/migrate_vsg_config.py old.yml > new.yml # a VSG 3.2x configuration, in 3.35's rule names
python scripts/learn_layout_rules.py FILE...           # relearn src/layout_rules.json
python scripts/gen_spacing_rules.py VSG_CHECKOUT/docs  # regenerate src/spacing_rules.json
python scripts/gen_rule_docs.py                        # regenerate the docs' rule tables
```

```sh
python scripts/corpus_findings.py --binary target/release/vsg-rs CORPUS...   # record
python scripts/corpus_findings.py --check --binary target/release/vsg-rs CORPUS...
```

The second is how the lint layer's claim is kept honest: it counts what vsg-rs's own rules report
over whole corpora and compares that with `tests/corpus-findings.txt`. A count that goes up is a
rule saying something new, and it has to be looked at before it is accepted; a count that goes
down is an improvement worth recording. Three rules were once found reporting false positives on
code they had never been run over, because each had been checked against a subset of a corpus.

`.github/workflows/compatibility.yml` runs the comparison weekly over two corpora — VUnit, and
open-logic for its 825 configured rules — and against whatever VSG released most recently.

## What CI runs

Beyond format, clippy and tests: `cargo deny check` (licences, duplicate crates, advisories;
`deny.toml`), `cargo machete` (unused dependencies), `typos` (`_typos.toml`), `cargo test --doc`,
`cargo llvm-cov` (coverage in the job summary), `cargo semver-checks` against the base branch
(reported, not enforced), `examples/bench` against the base branch (`scripts/compare_bench.py`,
warns when something is more than 25% slower), and, for the documentation site, `mkdocs build --strict`
plus `scripts/gen_rule_docs.py --check`, which fails when the generated rule tables no longer
match the binary. The formatter is fuzzed nightly (`fuzz/fuzz_targets/`).

## Using the library

`vsg_rs` can be used directly: `Parsed::new`, `format_parsed`, `fix_with`, `rules::check_with`
and the range variants `format_range` / `fix_range`.

## Releasing

See [docs/releasing.md](docs/releasing.md), including which VSG version a release targets and
the three places that must agree on it.

## Licence

By contributing you agree that your work is dual licensed under Apache-2.0 and MIT, as the
project is.

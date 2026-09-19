# Performance

Measured September 2026 on a 24-core x86-64 Linux machine, release build (`lto = "thin"`),
one binary, warm page cache. Each figure is the best of the stated number of runs, measured
process start to exit.

These numbers describe one machine on one day. Re-measure before quoting them: `examples/bench`
and `examples/corpus.rs` produce them, and `scripts/compare_bench.py` compares two runs.

## Editor path (`--stdin`)

Best of 30 runs, process start to exit, including parsing, layout, output verification
(a second parse) and writing stdout:

| Input | Time |
|---|---|
| `vsg-rs --version` (cold start) | 0.5 ms |
| 10-line entity/architecture | 1.1 ms |
| VUnit `axi_stream_pkg.vhd` (1,100 lines, 42 kB) | 16 ms |
| `vsg-rs --stdin --fix` on the same file | 28 ms |

## Library (`cargo run --release --example bench`)

Best of 3–10 runs, single thread. `format` includes output verification and alignment. `fix`
includes rule checks before and after, one parse of the fixed source, and formatting. The
`Lint` column is the style layer's rule checks over a parsed file, not the `--check lint` layer.

| Input | Bytes | Parse | Format | Lint | Fix |
|---|---|---|---|---|---|
| small file | 299 | 0.02 ms | 0.12 ms | 0.04 ms | 0.23 ms |
| 5,000 statements with trailing comments | 338 k | 30 ms | 160 ms | 25 ms | 244 ms |
| fold-heavy (2,000 long lines) | 489 k | 30 ms | 146 ms | 207 ms | 223 ms |
| call nesting depth 300 | 6 k | 1.8 ms | 7 ms | 10 ms | 11 ms |
| 1,000 generics + 1,000 ports | 31 k | 5 ms | 22 ms | 32 ms | 38 ms |
| 2,000-term boolean chain | 36 k | 5 ms | 22 ms | 31 ms | 34 ms |
| 10,000-element aggregate | 59 k | 6 ms | 30 ms | 39 ms | 43 ms |
| 20,000-line architecture | 1.4 M | 165 ms | 770 ms | 132 ms | 1.18 s |

Lint is more expensive on files with long lines, because `length_001` formats the file to find
out which overflows the formatter can fold.

## Repository

* 11,753 real-world files (134.6 MB): parse, format and verify in 39 s on one core
  (`examples/corpus.rs`), about 3.4 MB/s.
* `vsg-rs -f` on the files of VUnit's `vunit/vhdl` (222 files, 3.4 MB, including OSVVM):
  0.59 s wall and 3.7 s CPU with worker processes, against 2.8 s on one core. With threads
  in one process it took 1.7 s wall and 30 s CPU (see below). The rest is dominated by the
  largest file (OSVVM `CoveragePkg.vhd`, 436 kB, 0.4 s).

## Parallelism

`vhdl_syntax` interns every token text in one global `RwLock`, and reads it for every token
text or length, including during tree navigation. Threads in one process therefore contend on
every token: with 24 threads, 222 files took 1.7 s instead of 2.8 s, while using 30 s of CPU.
`vsg-rs` instead starts worker processes (the same executable, with the same arguments),
each with its own interner and one thread. Files are distributed by size (at most one process
per 64 KiB of input and per `-p` job). Workers first collect the cross-file declarations, then
check or fix their files and send the results back as JSON; the parent writes files and
reports in input order. If a worker cannot be started, everything runs in the parent process.
`--debug` prints the number of processes used.

Formatting and checking scale linearly with file size (CoveragePkg concatenated 1×, 2×, 4×:
format 0.21 s, 0.50 s, 1.04 s; check 0.30 s, 0.63 s, 1.39 s).

## Complexity

Layout is linear in the input: each group is decided once, and the fit test looks ahead at most
the remaining width. Token neighbours are looked up in a per-snapshot token array. The underlying
tree's sibling navigation is linear in the number of siblings, which made a first version
quadratic on long lists (10,000-element aggregate: 10 s, now 30 ms). An upstream improvement to
`vhdl_syntax` would remove the need for the array.

## Known costs

* `check` formats each file once (the result is shared by `length_001` and the format check);
  `fix` formats the fixed source once.
* Output verification parses the output a second time. It stays enabled because it is what
  makes format-on-save safe. Trailing-comment alignment reuses that parse.
* Diagnostics map offsets to lines with a per-snapshot line index.

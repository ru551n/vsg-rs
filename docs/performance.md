# Performance

One machine on one day: 24-core x86-64 Linux, release build, warm cache, September 2026.
Re-measure before quoting — `examples/bench` and `examples/corpus.rs` produce these.

| | |
|---|---|
| `--stdin` on a 1,100-line file | 16 ms, or 28 ms with `--fix` |
| `vsg-rs -f` over VUnit's VHDL (222 files, 3.4 MB) | 0.59 s |
| `vsg-rs lint` over VUnit's VHDL (493 files) | 0.59 s |
| parse + format + verify, 11,753 files (134.6 MB), one core | 39 s |

Fast enough to run on every save; the editor path is the one to watch, and it is the
first row.

**Why worker processes, not threads.** `vhdl_syntax` interns every token text in one
global `RwLock`, read again for every token length and every step through the tree. 24
threads in one process ran those 222 files in 1.7 s while burning 30 s of CPU — slower
per core than running them one at a time. `vsg-rs` starts worker processes instead, each
with its own interner, and distributes files by size. `--debug` prints how many it used.

**Known costs.** `check` formats each file once, because `length_001` needs to know what
the formatter can fold. Output is parsed a second time to verify it; that stays on,
because it is what makes format-on-save safe.

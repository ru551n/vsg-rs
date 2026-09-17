#!/usr/bin/env python3
"""Compare two runs of `cargo run --release --example bench` (the `bench` job in CI).

    compare_bench.py BASE.txt HEAD.txt [--threshold 25]

Writes a table of the times per input and phase to the job summary, and prints a workflow
warning for every phase that is more than THRESHOLD percent slower than the base. Runners are
shared machines, so this warns; it never fails the job.
"""

from __future__ import annotations

import argparse
import os
import re
import sys

UNITS = {"ns": 1e-6, "µs": 1e-3, "us": 1e-3, "ms": 1.0, "s": 1e3}
PHASES = ["parse", "format", "lint", "fix"]


def read(path: str) -> dict[str, list[float]]:
    """{input name: [milliseconds per phase]} from a bench table."""
    rows = {}
    for line in open(path, encoding="utf-8"):
        found = re.findall(r"([\d.]+)(ns|µs|us|ms|s)\b", line)
        if len(found) != len(PHASES):
            continue
        name = line[: line.index(found[0][0])].rsplit("  ", 1)[0].strip()
        name = re.sub(r"\s+\d+$", "", name)
        rows[name] = [float(value) * UNITS[unit] for value, unit in found]
    return rows


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("base")
    parser.add_argument("head")
    parser.add_argument("--threshold", type=float, default=25.0)
    args = parser.parse_args()
    base, head = read(args.base), read(args.head)
    if not base or not head:
        print("::warning title=bench::no benchmark results to compare")
        return 0

    lines = [
        "## Benchmarks",
        "",
        "Times of this pull request against its base, in milliseconds.",
        "",
    ]
    lines += ["| Input | Phase | Base | This PR | Change |", "|---|---|---|---|---|"]
    slower = []
    for name, times in head.items():
        if name not in base:
            continue
        for phase, before, after in zip(PHASES, base[name], times):
            change = (after - before) / before * 100 if before else 0.0
            lines.append(f"| {name} | {phase} | {before:.3f} | {after:.3f} | {change:+.1f}% |")
            # Sub-millisecond phases are noise on a shared runner.
            if change > args.threshold and after > 1.0:
                slower.append(f"{name}/{phase} {before:.3f} -> {after:.3f} ms ({change:+.1f}%)")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
    for line in slower:
        print(f"::warning title=Slower than the base branch::{line}")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())

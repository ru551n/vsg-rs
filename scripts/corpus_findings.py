#!/usr/bin/env python3
"""Count what the native analysis rules report over a corpus, and compare with what is expected.

The lint layer's claim is that a finding is proven, and the way that claim is kept honest is by
running the rules over real code and looking at everything they say. Three separate rules were
found reporting false positives on code they had never been run over, because each had been
validated against a subset of a corpus rather than the whole of it.

    python scripts/corpus_findings.py --binary target/release/vsg-rs CORPUS...
    python scripts/corpus_findings.py --check ...   # fail if the counts moved

Only vsg-rs's own rules are counted (`lint_6xx` and `lint_7xx`). The front end's rules are not:
they depend on a library map the corpora do not all carry, and they are not ours to judge.

Expectations live in `tests/corpus-findings.txt`. A count that goes up is a regression. A count
that goes down is an improvement, and updating the file is how it gets recorded.
"""

from __future__ import annotations

import argparse
import collections
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXPECTED = ROOT / "tests" / "corpus-findings.txt"
# vsg-rs's own rules. The front end's (lint_0xx to lint_5xx) are a different question.
NATIVE = re.compile(r"\blint_([67]\d\d)\b")


def findings(binary: pathlib.Path, corpus: pathlib.Path, config: pathlib.Path) -> dict[str, int]:
    """How many findings each native rule reports over one corpus."""
    out = subprocess.run(
        [
            str(binary.resolve()),
            "--recursive",
            ".",
            "--check",
            "lint",
            "-c",
            str(config.resolve()),
        ],
        cwd=corpus,
        capture_output=True,
        text=True,
    ).stdout
    counts: collections.Counter[str] = collections.Counter()
    for line in out.splitlines():
        # A report row, not a mention in prose: the rule stands in its own column.
        match = NATIVE.match(line.strip())
        if match:
            counts[f"lint_{match.group(1)}"] += 1
    return dict(counts)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpora", nargs="+", type=pathlib.Path)
    parser.add_argument("--binary", type=pathlib.Path, default=ROOT / "target/release/vsg-rs")
    parser.add_argument("--check", action="store_true", help="fail if the counts moved")
    parser.add_argument("--summary", type=pathlib.Path, help="write a table here as well")
    args = parser.parse_args()

    # Every rule, including the ones that are off by default: a rule nobody runs is a rule
    # nobody notices reporting nonsense.
    config = ROOT / "tests" / "corpus-findings.yaml"

    measured: dict[str, dict[str, int]] = {}
    for corpus in args.corpora:
        measured[corpus.name] = findings(args.binary, corpus, config)

    lines = []
    for corpus in sorted(measured):
        for rule in sorted(measured[corpus]):
            lines.append(f"{corpus} {rule} {measured[corpus][rule]}")
    report = "\n".join(lines) + "\n"

    if args.summary:
        table = ["| Corpus | Rule | Findings |", "|---|---|---|"]
        table += [f"| {line.split()[0]} | {line.split()[1]} | {line.split()[2]} |" for line in lines]
        args.summary.write_text("\n".join(table) + "\n")

    if not args.check:
        EXPECTED.write_text(report)
        print(report, end="")
        print(f"wrote {EXPECTED.relative_to(ROOT)}", file=sys.stderr)
        return 0

    expected = EXPECTED.read_text() if EXPECTED.exists() else ""
    if report == expected:
        print(report, end="")
        return 0
    print("the native rules no longer report what they used to.\n", file=sys.stderr)
    print("expected:\n" + expected, file=sys.stderr)
    print("measured:\n" + report, file=sys.stderr)
    print(
        "A count that went up is a rule reporting something new: check it is real before\n"
        "accepting it. A count that went down is an improvement. Either way, record it with\n"
        "  python scripts/corpus_findings.py --binary <binary> <corpora>",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())

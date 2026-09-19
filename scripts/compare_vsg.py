#!/usr/bin/env python3
"""Compare vsg-rs with VSG on a set of VHDL files.

Both tools run without --fix and write VSG's JSON report (-js). The findings are compared per
rule as (file, line, rule) triples. VSG runs with all phases (-ap) so that it reports as much as
vsg-rs does.

    python scripts/compare_vsg.py [--vsg "uvx --from vsg==3.35.0 vsg"]
        [--vsg-rs target/release/vsg-rs] [-c config.yaml] [--jobs N] [--out DIR] FILE...

Prints, per rule, the findings both tools report, only VSG, and only vsg-rs. For VSG's layout
rules, the last column counts VSG findings on lines that vsg-rs reports as `format`. The raw
reports and the differing findings are written to DIR (default: .compare).
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path


def run_json(cmd: list[str], files: list[str], extra: list[str]) -> dict:
    with tempfile.TemporaryDirectory() as tmp:
        out = os.path.join(tmp, "report.json")
        subprocess.run(
            [*cmd, "-f", *files, "-js", out, *extra],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if not os.path.exists(out):
            return {"files": []}
        with open(out) as f:
            return json.load(f)


def findings(report: dict) -> set[tuple[str, int, str]]:
    return {
        (os.path.normpath(f["file_path"]), v["linenumber"], v["rule"])
        for f in report["files"]
        for v in f["violations"]
    }


def layout_rules(vsg_rs: str) -> set[str]:
    listing = subprocess.run(
        [vsg_rs, "--list_rules"], capture_output=True, text=True, check=True
    ).stdout
    return {line.split()[0] for line in listing.splitlines() if "formatter" in line.split()[1:2]}


def vsg_version(command: str) -> str:
    """What `vsg --version` says, so a report names the version it was measured against."""
    try:
        out = subprocess.run(
            shlex.split(command) + ["--version"],
            capture_output=True,
            text=True,
            check=False,
            timeout=120,
        )
        text = (out.stdout + out.stderr).strip().splitlines()
        return text[0].strip() if text else "VSG (version unknown)"
    except (OSError, subprocess.SubprocessError):
        return "VSG (version unknown)"


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--vsg", default="uvx --from vsg==3.35.0 vsg")
    parser.add_argument("--vsg-rs", default="target/release/vsg-rs")
    parser.add_argument("-c", "--config", action="append", default=[])
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    parser.add_argument("--out", default=".compare")
    parser.add_argument(
        "--markdown",
        help="also write a summary table here (the GitHub job summary, for instance)",
    )
    parser.add_argument("files", nargs="+")
    args = parser.parse_args()

    files = sorted({os.path.normpath(f) for f in args.files})
    config = ["-c", *args.config] if args.config else []
    vsg: dict = {"files": []}
    for start in range(0, len(files), 50):
        part = run_json(
            shlex.split(args.vsg),
            files[start : start + 50],
            [*config, "-ap", "-p", str(args.jobs)],
        )
        vsg["files"].extend(part["files"])
    ours = run_json([args.vsg_rs], files, config)

    theirs_set, ours_set = findings(vsg), findings(ours)
    layout = layout_rules(args.vsg_rs)
    format_lines = {(f, line) for f, line, rule in ours_set if rule == "format"}

    rules = sorted({r for _, _, r in theirs_set | ours_set})
    rows = []
    for rule in rules:
        t = {x for x in theirs_set if x[2] == rule}
        o = {x for x in ours_set if x[2] == rule}
        covered = sum(1 for f, line, _ in t - o if (f, line) in format_lines)
        rows.append((rule, len(t & o), len(t - o), len(o - t), covered))

    width = max((len(r[0]) for r in rows), default=10)
    print(f"{'rule':{width}}  {'both':>6} {'vsg':>6} {'vsg-rs':>6} {'format':>6}")
    for rule, both, only_t, only_o, covered in rows:
        if only_t or only_o:
            print(f"{rule:{width}}  {both:6} {only_t:6} {only_o:6} {covered:6}")
    total: collections.Counter[str] = collections.Counter()
    for rule, both, only_t, only_o, covered in rows:
        if rule == "format":
            continue
        kind = "layout" if rule in layout else "rule"
        total[f"{kind}: both"] += both
        total[f"{kind}: VSG only"] += only_t
        total[f"{kind}: vsg-rs only"] += only_o
        total[f"{kind}: VSG only, on a vsg-rs format line"] += covered
    print()
    for k in sorted(total):
        print(f"{k}: {total[k]}")

    agreement = {}
    for kind in ("rule", "layout"):
        both, only_t, only_o = (
            total[f"{kind}: both"],
            total[f"{kind}: VSG only"],
            total[f"{kind}: vsg-rs only"],
        )
        seen = both + only_t + only_o
        agreement[kind] = 100.0 * both / seen if seen else 100.0
    if args.markdown:
        lines = [
            f"## Compatibility with {vsg_version(args.vsg)}",
            "",
            f"{len(files)} files.",
            "",
            "| Findings | Both | VSG only | vsg-rs only | Agreement |",
            "|---|---|---|---|---|",
        ]
        for kind in ("rule", "layout"):
            lines.append(
                f"| {kind} | {total[f'{kind}: both']} | {total[f'{kind}: VSG only']} "
                f"| {total[f'{kind}: vsg-rs only']} | {agreement[kind]:.1f}% |"
            )
        worst = sorted(rows, key=lambda r: -(r[2] + r[3]))[:15]
        lines += ["", "Rules that differ most:", "", "| Rule | Both | VSG only | vsg-rs only |", "|---|---|---|---|"]
        lines += [f"| `{r[0]}` | {r[1]} | {r[2]} | {r[3]} |" for r in worst if r[2] or r[3]]
        Path(args.markdown).write_text("\n".join(lines) + "\n")

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "vsg.json").write_text(json.dumps(vsg, indent=1))
    (out / "vsg-rs.json").write_text(json.dumps(ours, indent=1))
    details = {
        rule: {
            "vsg_only": sorted([f, n] for f, n, r in theirs_set - ours_set if r == rule),
            "vsg_rs_only": sorted([f, n] for f, n, r in ours_set - theirs_set if r == rule),
        }
        for rule in rules
    }
    (out / "details.json").write_text(json.dumps(details, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Report vsg-rs findings in the GitHub Action (`action.yml`).

    github_action_report.py --sarif FILE --workdir DIR --annotations {true,false,changed}
        --layout {suggestions,alerts} --pr-comment {true,false} [--diff FILE] [--command TEXT]

* Makes the SARIF file paths relative to the repository root (vsg-rs runs in DIR).
* With `--layout suggestions`, removes the layout results (`format`) from the SARIF file and,
  on pull requests, posts the changes `vsg-rs --fix` would make (`--diff FILE`) as suggested
  changes in one review, for the lines the pull request touches.
* Prints annotations: all findings, or on pull requests with `changed` only those on lines the
  pull request adds or changes.
* Writes a summary per rule to the job summary and, on pull requests with `--pr-comment true`,
  to one pull request comment that is updated on every run.

Uses only the standard library. The GitHub token is read from GITHUB_TOKEN.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import posixpath
import re
import sys
import urllib.error
import urllib.request

SUMMARY_MARKER = "<!-- vsg-rs summary -->"
SUGGESTION_MARKER = "<!-- vsg-rs suggestion -->"
MAX_SUGGESTIONS = 50
HUNK = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def api(method: str, path: str, body: object | None = None) -> object:
    url = os.environ.get("GITHUB_API_URL", "https://api.github.com") + path
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(url, data=data, method=method)
    request.add_header("Accept", "application/vnd.github+json")
    request.add_header("Authorization", f"Bearer {os.environ['GITHUB_TOKEN']}")
    request.add_header("X-GitHub-Api-Version", "2022-11-28")
    with urllib.request.urlopen(request) as response:
        text = response.read()
    return json.loads(text) if text else None


def paginate(path: str) -> list:
    items: list = []
    page = 1
    while True:
        sep = "&" if "?" in path else "?"
        batch = api("GET", f"{path}{sep}per_page=100&page={page}")
        items.extend(batch)
        if len(batch) < 100:
            return items
        page += 1


def warn(message: str) -> None:
    print(f"::warning title=vsg-rs::{message}")


def pull_request() -> dict | None:
    if os.environ.get("GITHUB_EVENT_NAME") not in ("pull_request", "pull_request_target"):
        return None
    try:
        with open(os.environ["GITHUB_EVENT_PATH"], encoding="utf-8") as f:
            return json.load(f).get("pull_request")
    except (KeyError, OSError, ValueError):
        return None


def pr_lines(repo: str, number: int) -> tuple[dict, dict]:
    """Per file: {line: text} of the lines a review comment can refer to, and the added lines."""
    visible: dict[str, dict[int, str]] = collections.defaultdict(dict)
    added: dict[str, set[int]] = collections.defaultdict(set)
    for f in paginate(f"/repos/{repo}/pulls/{number}/files"):
        line = 0
        for text in f.get("patch", "").splitlines():
            m = HUNK.match(text)
            if m:
                line = int(m.group(3))
                continue
            if text.startswith(("-", "\\")):
                continue
            visible[f["filename"]][line] = text[1:]
            if text.startswith("+"):
                added[f["filename"]].add(line)
            line += 1
    return visible, added


def diff_blocks(text: str, prefix: str) -> list[tuple[str, int, list[str], list[str]]]:
    """Changed blocks of a unified diff: (path, first old line, old lines, new lines)."""
    blocks = []
    path = None
    old_line = 0
    current = None
    for line in text.splitlines():
        if line.startswith("--- "):
            continue
        if line.startswith("+++ "):
            path = posixpath.normpath(posixpath.join(prefix, line[4:].replace("\\", "/")))
            continue
        m = HUNK.match(line)
        if m:
            old_line = int(m.group(1))
            if m.group(2) == "0":
                old_line += 1  # a pure insertion names the line before it
            current = None
            continue
        if path is None or line.startswith("\\"):
            continue
        if line.startswith(" "):
            current = None
            old_line += 1
            continue
        if current is None:
            current = [path, old_line, [], []]
            blocks.append(current)
        if line.startswith("-"):
            current[2].append(line[1:])
            old_line += 1
        elif line.startswith("+"):
            current[3].append(line[1:])
    return [tuple(b) for b in blocks]


def graphql(query: str, variables: dict) -> dict:
    url = os.environ.get("GITHUB_GRAPHQL_URL", "https://api.github.com/graphql")
    request = urllib.request.Request(
        url, data=json.dumps({"query": query, "variables": variables}).encode(), method="POST"
    )
    request.add_header("Authorization", f"Bearer {os.environ['GITHUB_TOKEN']}")
    with urllib.request.urlopen(request) as response:
        result = json.loads(response.read())
    if result.get("errors"):
        raise RuntimeError(result["errors"][0].get("message", "GraphQL error"))
    return result["data"]


THREADS = """
query($owner: String!, $name: String!, $number: Int!, $after: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100, after: $after) {
        nodes { id isResolved comments(first: 1) { nodes { body path line } } }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
"""


def sync_threads(repo: str, number: int, current: set) -> set:
    """Resolve the action's suggestion threads that vsg-rs no longer makes, reopen the ones it
    makes again, and return the (path, line, body) of all the action's threads."""
    owner, name = repo.split("/", 1)
    threads = []
    after = None
    while True:
        data = graphql(THREADS, {"owner": owner, "name": name, "number": number, "after": after})
        page = data["repository"]["pullRequest"]["reviewThreads"]
        threads += page["nodes"]
        if not page["pageInfo"]["hasNextPage"]:
            break
        after = page["pageInfo"]["endCursor"]
    existing = set()
    for thread in threads:
        first = thread["comments"]["nodes"][0] if thread["comments"]["nodes"] else None
        if not first or SUGGESTION_MARKER not in first["body"]:
            continue
        key = (first["path"], first["line"], first["body"])
        existing.add(key)
        wanted = key in current
        if wanted == thread["isResolved"]:
            mutation = "unresolveReviewThread" if wanted else "resolveReviewThread"
            try:
                graphql(
                    f"mutation($id: ID!) {{ {mutation}(input: {{threadId: $id}}) "
                    "{ thread { id } } }",
                    {"id": thread["id"]},
                )
            except (urllib.error.HTTPError, RuntimeError) as e:
                warn(
                    f"cannot resolve or reopen earlier suggestions ({getattr(e, 'code', e)}); "
                    "the job needs `contents: write` for that"
                )
                break
    return existing


def suggestions(repo: str, pr: dict, diff_text: str, prefix: str, visible: dict) -> int:
    comments = []
    for path, first, old, new in diff_blocks(diff_text, prefix):
        lines = visible.get(path, {})
        if not old:
            # A pure insertion: suggest it together with the line above.
            anchor = first - 1
            if anchor not in lines:
                continue
            first, old, new = anchor, [lines[anchor]], [lines[anchor], *new]
        last = first + len(old) - 1
        # Only lines shown in the pull request, and only if they still read as vsg-rs saw them.
        if any(lines.get(n) != text for n, text in zip(range(first, last + 1), old)):
            continue
        body = "\n".join(
            [SUGGESTION_MARKER, "vsg-rs would format this as:", "```suggestion", *new, "```"]
        )
        comment = {"path": path, "line": last, "side": "RIGHT", "body": body}
        if last > first:
            comment.update(start_line=first, start_side="RIGHT")
        comments.append(comment)
    current = {(c["path"], c["line"], c["body"]) for c in comments}
    existing = sync_threads(repo, pr["number"], current)
    comments = [c for c in comments if (c["path"], c["line"], c["body"]) not in existing]
    skipped = max(0, len(comments) - MAX_SUGGESTIONS)
    comments = comments[:MAX_SUGGESTIONS]
    if not comments:
        return 0
    body = f"vsg-rs: {len(comments)} formatting suggestion(s)."
    if skipped:
        body += f" {skipped} more are not shown; run vsg-rs with --fix."
    api(
        "POST",
        f"/repos/{repo}/pulls/{pr['number']}/reviews",
        {
            "commit_id": pr["head"]["sha"],
            "event": "COMMENT",
            "body": body,
            "comments": comments,
        },
    )
    return len(comments)


def summary_markdown(results: list, command: str, posted: int | None) -> str:
    counts: collections.Counter[tuple[str, str]] = collections.Counter(
        (r["ruleId"], r["level"]) for r in results
    )
    files = {r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"] for r in results}
    lines = ["## vsg-rs", ""]
    if not results:
        lines.append("No violations.")
        return "\n".join(lines) + "\n"
    lines.append(f"**{len(results)}** finding(s) in **{len(files)}** file(s).")
    lines += ["", "| Rule | Severity | Count |", "|---|---|---|"]
    for (rule, level), count in sorted(counts.items(), key=lambda x: (-x[1], x[0])):
        name = "layout (blocks to reformat)" if rule == "format" else f"`{rule}`"
        lines.append(f"| {name} | {level} | {count} |")
    if posted:
        lines += ["", f"{posted} formatting suggestion(s) were added to the review."]
    if command:
        lines += ["", "Fix locally with:", "", "```sh", f"vsg-rs {command} --fix", "```"]
    return "\n".join(lines) + "\n"


def update_pr_comment(repo: str, number: int, text: str, clean: bool) -> None:
    comments = [
        c
        for c in paginate(f"/repos/{repo}/issues/{number}/comments")
        if SUMMARY_MARKER in (c.get("body") or "")
    ]
    body = f"{SUMMARY_MARKER}\n{text}"
    if comments:
        api("PATCH", f"/repos/{repo}/issues/comments/{comments[0]['id']}", {"body": body})
    elif not clean:
        api("POST", f"/repos/{repo}/issues/{number}/comments", {"body": body})


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sarif", required=True)
    parser.add_argument("--workdir", default=".")
    parser.add_argument("--annotations", default="changed")
    parser.add_argument("--layout", default="suggestions")
    parser.add_argument("--pr-comment", default="true")
    parser.add_argument("--diff")
    parser.add_argument("--command", default="")
    args = parser.parse_args()

    with open(args.sarif, encoding="utf-8") as f:
        doc = json.load(f)
    prefix = posixpath.normpath(args.workdir.replace("\\", "/"))
    results = doc["runs"][0]["results"]
    for result in results:
        location = result["locations"][0]["physicalLocation"]["artifactLocation"]
        if not location["uri"].startswith("file://"):
            location["uri"] = posixpath.normpath(posixpath.join(prefix, location["uri"]))
    all_results = list(results)
    if args.layout == "suggestions":
        results[:] = [r for r in results if r["ruleId"] != "format"]
    with open(args.sarif, "w", encoding="utf-8") as f:
        json.dump(doc, f, indent=1)

    pr = pull_request()
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    visible: dict = {}
    added: dict = {}
    if pr and os.environ.get("GITHUB_TOKEN"):
        try:
            visible, added = pr_lines(repo, pr["number"])
        except urllib.error.HTTPError as e:
            warn(f"cannot read the pull request's files ({e.code}); reporting all findings")
            pr = None

    if args.annotations != "false":
        for result in all_results:
            location = result["locations"][0]["physicalLocation"]
            path = location["artifactLocation"]["uri"]
            region = location["region"]
            first, last = region["startLine"], region.get("endLine", region["startLine"])
            if (
                pr
                and args.annotations == "changed"
                and not (added.get(path, set()) & set(range(first, last + 1)))
            ):
                continue
            level = "warning" if result["level"] == "warning" else "error"
            message = result["message"]["text"].replace("%", "%25").replace("\n", "%0A")
            print(
                f"::{level} file={path},line={first},endLine={last},"
                f"title={result['ruleId']}::{message}"
            )

    posted = None
    if pr and args.layout == "suggestions" and args.diff:
        with open(args.diff, encoding="utf-8", errors="replace") as f:
            diff_text = f.read()
        try:
            posted = suggestions(repo, pr, diff_text, prefix, visible)
        except (urllib.error.HTTPError, RuntimeError) as e:
            code = getattr(e, "code", e)
            warn(
                f"cannot post or update suggestions ({code}); the job needs "
                "`pull-requests: write`, and pull requests from forks get a read-only token"
            )

    text = summary_markdown(all_results, args.command, posted)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write(text)
    if pr and args.pr_comment == "true":
        try:
            update_pr_comment(repo, pr["number"], text, clean=not all_results)
        except urllib.error.HTTPError as e:
            warn(f"cannot update the pull request comment ({e.code}); needs `pull-requests: write`")
    return 0


if __name__ == "__main__":
    sys.exit(main())

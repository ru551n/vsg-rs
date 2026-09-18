# GitHub Action

`ru551n/vsg-rs` is also a GitHub Action. It downloads the vsg-rs release binary (checked
against `SHA256SUMS`), runs it with VSG's arguments and reports the findings. On pull requests
it keeps the noise down:

* **Layout as suggested changes.** What `--fix` would change on the lines of the pull request
  is posted as suggested changes in one review, which can be applied with one click. Layout
  findings do not become code scanning alerts (unless `layout: alerts`).
* **One summary comment**, with the findings per rule and the command that fixes them,
  updated in place on every push (and not created while there is nothing to report).
* **Annotations only on changed lines** (`annotations: changed`).
* **Code scanning** (optional, see below) to track rule violations as alerts.

```yaml
name: VHDL style
on: [push, pull_request]

jobs:
  vsg:
    runs-on: ubuntu-latest
    permissions:
      contents: write          # only to resolve suggestions that no longer apply (else: read)
      pull-requests: write     # suggestions and the summary comment
    steps:
      - uses: actions/checkout@v6
      - uses: ru551n/vsg-rs@v0.10.0
        with:
          args: -c vsg.yaml --recursive src
```

It runs on Linux, Windows and macOS runners (x64 and arm64).

| Input | Default | Meaning |
|---|---|---|
| `args` | `--recursive .` | vsg-rs arguments (VSG's command line); `--sarif` is added |
| `version` | the action's tag, else `latest` | vsg-rs release to download |
| `working-directory` | `.` | where vsg-rs runs, relative to the repository root |
| `annotations` | `changed` | findings as annotations: `changed` (on pull requests, only on lines the pull request adds or changes), `true` or `false`; GitHub shows at most 10 errors and 10 warnings per step |
| `layout` | `suggestions` | layout findings as suggested changes on pull requests (`suggestions`), or as code scanning alerts, one per block of lines (`alerts`) |
| `pr-comment` | `true` | one summary comment on the pull request, updated on every run |
| `sarif-upload` | `false` | upload to code scanning (needs `security-events: write`) |
| `fail-on-violations` | `true` | fail the job when vsg-rs exits with 1 |
| `token` | `github.token` | token for downloading the release and for the pull request review and comment |

Outputs: `exit-code`, `sarif-file` (paths relative to the repository root) and `version`.

Suggestions and the summary comment need `pull-requests: write`. Pull requests from forks get
a read-only token; the action then only warns and reports through annotations and the job
summary. Suggestions refer to the pull request's head commit and are posted only for lines
that the pull request shows and that still read as vsg-rs saw them; at most 50 per run, and a
suggestion that was already posted is not repeated. On every run, the action resolves its
earlier suggestion threads that vsg-rs no longer makes (the problem was fixed, or the lines
changed) and reopens a resolved one when the same suggestion applies again, so only open
problems stay expanded. GitHub only lets a workflow resolve review threads with
`contents: write`; with `contents: read`, the action warns and leaves earlier suggestions as
they are (GitHub still collapses those whose lines changed). The action itself never pushes.

## Code scanning (SARIF), optional

The suggestions, the summary comment and the annotations need nothing but the workflow. Code
scanning is an addition for teams that want violations tracked over time: it adds GitHub's
code scanning bot (`github-advanced-security[bot]`), which comments once per new alert, and
a separate check. Enable it with `sarif-upload: true` and `security-events: write`.

SARIF is the standard JSON format for static-analysis results that GitHub code scanning reads.
With `sarif-upload: true` (or `vsg-rs --sarif FILE` followed by
`github/codeql-action/upload-sarif`), each violation becomes a code scanning alert:

* vsg-rs reports one result per rule violation, and one `format` result per block of adjacent
  lines with layout findings (naming the VSG rules involved), so a badly formatted region is one
  alert rather than one per line and rule.
* **Security → Code scanning** lists the alerts; they can be filtered by rule (`entity_008`,
  …) and severity, and dismissed with a reason.
* Alerts are tracked across commits: an alert that disappears is closed automatically, and a
  pull request shows only the alerts it introduces, as review annotations on the changed lines.
* A branch protection rule or ruleset can require that code scanning finds no new alerts.

Requirements: code scanning is free for public repositories; private repositories need GitHub
Advanced Security (GitHub Code Security). The job needs `security-events: write`. File paths in
the report must be relative to the repository root, which the action ensures. The uploads use
the category `vsg-rs`, so they do not replace the results of other tools such as CodeQL.

Without code scanning, the annotations and the job summary still show the findings, and the
SARIF file is available as the `sarif-file` output (for example to upload as an artifact).

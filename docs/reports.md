# Report formats

Every format is written alongside the console report, not instead of it, and any number of them
can be produced in one run. None of them changes the exit code.

| Option | Format | Read by |
|---|---|---|
| `--json FILE` | vsg-rs's own JSON | anything you script |
| `-j`, `--junit FILE` | JUnit XML | GitLab, Jenkins, Azure Pipelines, most CI test tabs |
| `--sarif FILE` | SARIF 2.1.0 | GitHub code scanning, Jenkins Warnings-NG, SonarQube |
| `--quality_report FILE` | Code Climate JSON | GitLab's merge request widget |
| `--sonarqube FILE` | SonarQube generic issues | SonarQube Server and Cloud |

## SonarQube

```sh
vsg-rs --recursive src --check style,lint --sonarqube sonar-issues.json
```

```properties
sonar.externalIssuesReportPaths=sonar-issues.json
```

SonarQube reads SARIF as well, and vsg-rs writes that too — but SonarQube files **every** issue
from a SARIF report as a vulnerability. A missing blank line is not a security finding, and a few
hundred of them would bury the ones that are. The generic format carries the type, so vsg-rs sets
it from the layer the finding came from:

* a **definite error** arrives as a **bug** — it says the design cannot do what it says;
* everything else arrives as a **code smell** — style and layout, which say the code reads
  badly, and any [advisory, experimental or policy rule](lint.md#how-certain-is-a-finding) you
  switched on, which states something exact whose significance is yours to judge.

The type follows what the rule can prove rather than which layer it came from. A rule reporting
a naming convention or a state nothing enters is not filing a bug, however deliberately it was
enabled — a report that hands out its severest category freely is a report nobody reads.

Severity separates the findings you have to think about from the ones you do not:

| Severity | What it is |
|---|---|
| `INFO` | `--fix` repairs it on its own — one run removes all of them at once |
| `MINOR` | the rule is configured as a warning |
| `MAJOR` | style that needs a person: a name, a port mode, an instantiation |
| `CRITICAL` | a lint finding — by default a definite error, and whatever else you asked for |

Fixability is asked of each finding rather than of its rule, because the same rule can offer a
safe fix in one place and none in another. On open-logic that splits 33,693 issues into 32,003
`INFO`, 653 `MAJOR`, 124 `MINOR` and 913 `CRITICAL` — and running `--fix` takes the `INFO` count
to zero while leaving every `MAJOR` in place, which is what makes the distinction worth having.

It is deliberately conservative: a finding is only `INFO` when a safe fix is attached to it, so
the report never hides something as trivial that is not. The exit code is unaffected — an `INFO`
violation still fails the run, it just does not pretend to be technical debt.

Columns are converted, because SonarQube counts them from zero and every other format here counts
from one.

## What each format carries about a rule's class

Every format says what it can. None of them is asked to carry a distinction it has no field for.

| Format | How the class appears |
|---|---|
| SonarQube | `type`: a definite error is a `BUG`, everything else a `CODE_SMELL` |
| GitLab code quality | `categories`: `Bug Risk` for a definite error, `Clarity` for advisory and experimental, `Style` for policy and the style layer |
| SARIF | a tag on the rule, so a consumer that groups by tag can separate them |
| Console, JSON, JUnit | the rule id, which `--explain` and `--list_rules` describe |

Severity is not the class. An enabled rule reports at `error` and fails the build whatever its
class, because switching a rule on is asking for it to be enforced. Set `severity: warning` on
the rule or its group to report without failing.

## SARIF

```sh
vsg-rs --recursive src --check style,lint --sarif vsg-rs.sarif
```

Findings carry more than a position:

* **`relatedLocations`** — the other places a finding is about. `lint_601` lists each driver of
  the signal; `lint_720` lists each signal on the cycle. Code scanning links them, so a multiple
  driver is two places you can click rather than two numbers in a sentence.
* **`fixes`** — the edits of a safe fix, as regions and replacement text. Only fixes vsg-rs would
  apply itself are offered, so a fix suggested in a review matches what `--fix` does. Fixes VSG
  does not apply by default (`--unsafe_fixes`) are never emitted, and a lint finding never carries
  one, because applying it would change what the design does.

Each fix is independently applicable, as SARIF intends: applying one may leave others to report.

## Jenkins

Use the SARIF file. The Warnings Next Generation plugin has a SARIF parser, so no vsg-rs-specific
format is needed:

```groovy
recordIssues tool: sarif(pattern: 'vsg-rs.sarif')
```

## GitHub and GitLab

Both have a page of their own: [GitHub Action](github-action.md) uploads SARIF to code scanning
and posts suggested changes, and [GitLab CI](gitlab-ci.md) uses the code-quality report and JUnit.

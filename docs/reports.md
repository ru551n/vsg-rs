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

* the lint layer (`lint_*`) arrives as a **bug** — it says the hardware is wrong;
* style and layout arrive as a **code smell** — they say it reads badly.

Severity follows the rule's own: an error becomes `MAJOR`, a warning `MINOR`. Columns are
converted, because SonarQube counts them from zero and every other format here counts from one.

## Jenkins

Use the SARIF file. The Warnings Next Generation plugin has a SARIF parser, so no vsg-rs-specific
format is needed:

```groovy
recordIssues tool: sarif(pattern: 'vsg-rs.sarif')
```

## GitHub and GitLab

Both have a page of their own: [GitHub Action](github-action.md) uploads SARIF to code scanning
and posts suggested changes, and [GitLab CI](gitlab-ci.md) uses the code-quality report and JUnit.

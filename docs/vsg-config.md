# VSG Configuration Model (v3.35.0)

Own-words summary of how VSG is configured and invoked, compiled from
`configuring_overview.rst`, `configuring_length_rules.rst`, a JSON dump of
every rule's live configuration (`all_rules_config.yaml`, produced by
`vsg -oc`), and black-box verification against the real `vsg==3.35.0`
binary via `uvx` (no vsg-rs source touched, no Rust built). No VSG prose or
example code was copied verbatim; the example configs and VHDL below are
written from scratch for this note.

## 1. Config file basics

A VSG configuration file is plain **JSON or YAML** (VSG parses either;
YAML is just sugar over the same JSON-shaped document — the docs
consistently show the two as interchangeable). It is never auto-discovered:
it must be passed explicitly with `-c`/`--configuration`, which accepts one
or more file paths. I verified this black-box: dropping a `vsg.yaml` (and
separately a `.vsg.yaml`) with `rule: {global: {disable: true}}` into the
working directory and running `vsg -f <file>` with **no** `-c` flag left
the reported violations completely unchanged — VSG only reads what `-c`
points at, there is no implicit "project config file" convention like
`.eslintrc`/`rustfmt.toml`. When multiple `-c` files are given, or when
`rule.global`/`rule.group`/`rule.<id>`/`file_list`-scoped settings for the
same rule collide, priority is documented as, from lowest to highest:
`rule.global` < `rule.group.<group_name>` < `rule.<ruleId_ruleNumber>` <
a per-file override under `file_list`/`file_rules`. In other words: the more
specific the key path, the more it wins, and a per-file override always
wins over everything global.

## 2. Top-level keys

From the overview doc plus what `-oc`'s full dump actually contains, a
config document can use these top-level keys (all optional, order doesn't
matter, any subset may be present):

| Key | Purpose |
|---|---|
| `file_list` | List of file paths/globs to analyze. Env vars are expanded before globbing; paths are normalized to `/`-separated POSIX style regardless of platform. A list entry can be a plain path/glob string, or a `{path: {rule: {...}}}` map to attach per-file rule overrides inline (documented as deprecated in favor of `file_rules`). |
| `file_rules` | Same per-file `{path: {rule: {...}}}` override shape as the deprecated `file_list` form, but for files that are *not* thereby added to the scan list — i.e. "if this file happens to be analyzed (via `file_list` or `-f`), apply these extra rule settings to it," decoupled from file selection. |
| `local_rules` | Path to a directory of user-authored Python rule plugins. Can also be given on the CLI (`-lr`); if both are given, the config file's value wins. |
| `linesep` | Output line separator override. Values are `"\n"` or `"\r\n"`; default is platform-native if omitted. |
| `rule` | The main rule-configuration block (see below). |
| `indent` | A shared indentation-engine config block, independent of `rule`. Verified real (not just documentary): a minimal `indent: {tokens: {...}}` YAML file loads without error against `vsg==3.35.0` and its `tokens` structure matches what `vsg -oc` dumps under `indent`. |
| `pragma` | Config for the regexes VSG uses to recognize simulation on/off pragma comments (e.g. `-- synthesis translate_off`/`_on`), as `pragma.patterns.{open,close,single}` lists of regex strings. Only seen in the `-oc` dump, not in the prose docs pulled for this pass — treat as a real but rarely hand-edited knob. |

`file_list`, `local_rules`, and `rule` are each independently optional —
a config can set just one of them.

## 3. The `rule` block

```yaml
rule:
  global:
    <attribute>: <value>        # applies to every rule unless overridden
  group:
    <group_name>:
      <attribute>: <value>      # applies to every rule in that named group
  <ruleId>_<ruleNumber>:
    <attribute>: <value>        # applies to exactly one rule, e.g. entity_004
```

Every rule is addressed by its `<ruleId>_<ruleNumber>` name exactly as
listed in `vsg-rules.md` (e.g. `whitespace_006`, `port_010`, `length_001`).
`group` targets one of VSG's named rule groups (`indent`, `case`,
`alignment`, `blank_line`, `naming`, `structure`, ... — the same groupings
used as the "owner" hint in `vsg-rules.md`), letting you flip a whole
category (e.g. "disable every alignment rule") in one line instead of
listing every member rule. If `global` and a specific rule/group both set
the same attribute, the more specific one wins (see priority order above).

### Attributes every rule supports

Confirmed directly from `vsg -rc <any rule>` and from `all_rules_config.yaml`
(every one of the 972 rule entries carries these five keys):

| Attribute | Type | Meaning |
|---|---|---|
| `disable` | bool | Turns the rule off entirely (no report, no fix). |
| `fixable` | bool | Whether `--fix` is allowed to auto-correct this rule's violations (independent of whether the rule *can* fix itself — see `length_001` below, which is permanently `fixable: false`). |
| `phase` | int (1-7) | Which of VSG's seven fix/report phases the rule runs in (see `vsg-rules.md` for the phase table). Rules stop being *reported* once an earlier phase has violations, unless `-ap`/`--all_phases` is passed. |
| `severity` | `"Error"` \| `"Warning"` | Report severity; does not by itself affect the process exit code (both still count as violations) but affects the printed summary counts and JUnit/JSON export. |
| `indent_size` | int | Spaces per indent level for that rule's own indentation math (defaults to 2 almost everywhere in the dump). |
| `indent_style` | `"spaces"` (only value seen) | Present on every rule but effectively fixed. |
| `user_error_message` | string | Custom text appended to the rule's reported message; empty by default. |

### Attributes specific to a rule's category

Beyond the universal set, each rule adds whatever extra knobs its specific
check needs. Surveying `all_rules_config.yaml` by category:

- **Case rules** (`*_500` and friends, e.g. `entity_004`, `signal_004`,
  `port_010`): add `case` (`"lower"` or `"upper"`, default `"lower"` for
  nearly all keyword/identifier case rules), plus, for identifier-case
  rules specifically, `case_exceptions`, `prefix_exceptions`, and
  `suffix_exceptions` (lists of literal strings/identifiers to skip when
  checking case — this is the "exception list" mechanism that VSG-BUG-018
  in `upstream-bugs.md` found to be inconsistently honored).
- **Naming/prefix/suffix rules** (e.g. `signal_008`, `variable_012`,
  `type_600`): add `prefixes`/`suffixes` (lists, e.g. `signal_008` defaults
  to `["s_"]`) and `exceptions`; these rules default to `disable: true`
  (i.e. VSG ships prefix/suffix conventions off by default — a project
  opts in).
- **Regex-based naming rules** (e.g. `signal_004`): add a `regex` string
  attribute (empty by default), letting a project require identifiers to
  match a pattern instead of / in addition to a fixed prefix.
- **Alignment rules** (`*_400`, e.g. `declarative_part_400`,
  `variable_assignment_400`): add `compact_alignment` (`"yes"`/`"no"`),
  `blank_line_ends_group`, and `comment_line_ends_group` (all `"yes"`/`"no"`)
  — these control whether a blank line or a full-line comment breaks a
  contiguous block of declarations into separate alignment groups (directly
  relevant to VSG-BUG-030's alignment-drift finding, since these three
  options define what "one alignment group" even means).
- **Length rules** (`length_001`/`002`/`003`): add a single `length`
  integer attribute. Defaults: `length_001` (max line length) = **120**,
  `length_002` (max file length) = **2000**, `length_003` (max process
  length) = **500**. All three are permanently `"fixable": false` — VSG
  never rewraps lines automatically (see `upstream-limitations.md` L3/L
  VSG-BUG-026); a project can only move the threshold, never make the
  violation self-heal.

### `indent.tokens` (the shared indent engine)

Separately from per-rule `indent_size`, the `indent` top-level key
configures a shared table (`indent.tokens`, 84 groups in the current
release, one per grammar construct such as `architecture_body`,
`assertion_statement`, `alias_declaration`, ...) that says, for a specific
keyword/token inside that construct, how many indent levels *before* it
(`token`) and *after* it (`after`) apply. Values are either a small integer
(an absolute indent-level delta), or the strings `"current"` (no change
from the surrounding level) or `"+1"`/`"-1"` (relative to the surrounding
level). Example straight from the dump (paraphrased structurally, not
copied prose):

```json
"architecture_body": {
  "architecture_keyword": { "token": 0, "after": 1 },
  "begin_keyword":        { "token": 0, "after": 1 },
  "end_keyword":           { "token": 0, "after": 0 }
}
```
reads as: the `architecture`/`begin`/`end` keywords themselves sit at the
current level (`token: 0`), while everything *after* `architecture` and
`begin` steps in one level (`after: 1`), and nothing steps in after `end`.
There is also a small `indent.options` block (currently just
`comment.align_with_end_of_declarative_part` /
`align_with_end_of_statement_part`, both boolean) controlling a couple of
comment-indent special cases. This is the lowest-level, most mechanical
part of VSG's configuration surface, and the one that would need the
richest native equivalent in a formatter-first redesign (see closing
notes).

## 4. A config file I wrote (own example)

```yaml
# team-style.yml — example only, not derived from any VSG example
linesep: "\n"

rule:
  global:
    indent_size: 4          # this shop indents 4 spaces, not VSG's default 2

  group:
    naming:
      disable: true          # skip all prefix/suffix naming conventions

  entity_004:
    case: upper               # entity names must be UPPERCASE

  signal_004:
    case: lower
    prefix_exceptions: ["g_"] # generic-derived signal aliases exempted

  length_001:
    length: 100                # tighter line-length cap than the 120 default

file_rules:
  - legacy/old_core.vhd:
      rule:
        length_001:
          disable: true        # legacy file is exempt from the line-length cap
```

Invoked as: `vsg -f src/*.vhd -c team-style.yml`.

## 5. CLI surface (`vsg --help`, v3.35.0, verified via `uvx --from vsg==3.35.0 vsg --help`)

Actual current flags (some differ from commonly-assumed spellings — see
note below):

| Flag | Long form | Meaning |
|---|---|---|
| `-f FILENAME...` | `--filename` | File(s)/patterns to analyze (also accepted as bare positional args). |
| `-lr DIR` | `--local_rules` | Path to a directory of custom Python rule plugins. |
| `-c FILE...` | `--configuration` | One or more JSON/YAML config files (see above). |
| — | `--fix` | Apply fixes in place, instead of just reporting. |
| `-fp N` | `--fix_phase` | Only fix up through phase N (not all 7). |
| `-j FILE` | `--junit` | Write a JUnit XML report. |
| `-js FILE` | `--json` | Write a JSON report. |
| `-of {vsg,syntastic,summary}` | `--output_format` | Console report format. |
| `-b` | `--backup` | Keep a `.bak`-style copy of the pre-fix file. |
| `-oc FILE` | `--output_configuration` | Dump the full effective config (every rule, every attribute) to FILE — this is exactly the file this research used as `all_rules_config.yaml`. |
| `-rc RULEID` | `--rule_configuration` | Print just one rule's current config (JSON) to stdout. |
| — | `--style {indent_only,jcl}` | Load one of VSG's two built-in named presets before applying `-c` overrides. |
| `-v` | `--version` | Print version and exit (exit 0). |
| `-ap` | `--all_phases` | Don't stop reporting at the first phase with violations; report all 7. Mutually exclusive with `--fix` (verified: combining them is a CLI usage error, exit 1, `"-ap argument is invalid with the --fix argument"`). |
| — | `--fix_only FILE` | Restrict which rules `--fix` is allowed to touch, via a JSON allow-list file. |
| — | `--stdin` | Read one file's VHDL from stdin; disables file selection and multiprocessing. |
| — | `--force_fix` | Alpha: apply fixes even if syntax errors are present (see `upstream-limitations.md` L7 — this doesn't guarantee the *output* stays valid). |
| — | `--quality_report FILE` | GitLab code-quality JSON report. |
| — | `--sonarqube FILE` | SonarQube generic issue JSON (`sonar.externalIssuesReportPaths`); see [reports](reports.md). |
| `-p N` | `--jobs` | Parallel worker count (default: CPU core count). |
| — | `--debug` | Verbose internal debug output. |

**Correction vs. the task's assumed flag set**: there is no `--all-phases`
(dash form) or `-p`/`--fix-phase` combination in the real CLI — the actual
pairing is `-fp`/`--fix_phase` for "fix up to phase N" and separately
`-p`/`--jobs` for parallelism (an unrelated option that happens to share the
`-p` short flag with what one might guess `--fix_phase` abbreviates to);
`--all_phases`/`-ap` uses underscores like the rest of VSG's long-option
naming, not hyphens.

### Exit codes (black-box verified against a real `vsg==3.35.0`, not assumed)

| Scenario | Exit code |
|---|---|
| File analyzed, **zero** violations | `0` |
| File analyzed, **any** violation found (even only `Warning` severity ones going by the same code path used for `Error`) | `1` |
| `--fix` run that successfully brings the file to zero remaining violations | `0` |
| Invalid CLI usage (e.g. nonexistent `-f` file, or `-ap` combined with `--fix`) | `1` (argparse-style usage error, no traceback) |
| `-v`/`--version` | `0` |

So the exit code is a simple "clean vs. not-clean-or-broken" boolean, not a
graded signal — a caller cannot distinguish "1 style warning" from "internal
crash" from "bad arguments" by exit code alone; only the printed output
distinguishes them. Worth deciding deliberately for vsg-rs whether to keep
this or add distinct exit codes (e.g. 0 clean / 1 violations found / 2 usage
error / 3 internal error), which is a strictly more useful contract for
CI/tooling and costs nothing architecturally.

## 6. Config discovery summary

- VSG has **no default config filename** and **no config file
  auto-discovery** of any kind (no walking up parent directories, no
  `.vsg.yaml`/`vsg.yaml`/`pyproject.toml`-style convention). Verified
  black-box (see Section 1).
- The only way a config is read is an explicit `-c`/`--configuration` path
  on the command line, and the only way a *style preset* is loaded is the
  explicit `--style {indent_only,jcl}` flag.
- This is a real, easy usability upgrade opportunity for vsg-rs: adopting a
  conventional discovery order (e.g. `--config` flag > `.vsg-rs.toml` in cwd
  > walk up to repo root) costs little and removes a real point of friction
  every current VSG user has to solve themselves (wrapper scripts, Makefile
  targets, pre-commit hook args, etc., all just to pass `-c` every time).

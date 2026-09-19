# Compatibility with VSG

vsg-rs reads VSG configuration files and uses VSG rule identifiers, but it is an independent
implementation with a different architecture. This page lists what carries over, what does not,
and where vsg-rs deliberately differs. The VSG behaviour described here is based on VSG 3.35.0
documentation and black-box runs (see `vsg-config.md`).

## Configuration files

vsg-rs accepts VSG configuration documents in YAML or JSON.

| VSG key | vsg-rs |
|---|---|
| `rule.global`, `rule.group.<name>`, `rule.<id>` | supported, with the same precedence: global < group < rule |
| `disable`, `severity`, `fixable` | supported for implemented rules |
| `phase` | accepted and ignored (vsg-rs has no phases) |
| `user_error_message` | accepted and ignored |
| `rule.length_001.length` | sets the formatter's target width and the `length_001` limit |
| `rule.global.indent_size` | sets the formatter's indentation |
| `rule.global.indent_style` | `spaces` (default) or `smart_tabs` |
| `number_of_spaces`, port mode `spaces_before`/`spaces_after`, `action: same_line` of closing parentheses, `align_left`/`align_paren` | applied by the formatter (see `formatting.md`) |
| `rule.global.case`, `rule.group.case` / `case::keyword` `.case` | sets keyword case (`lower` or `upper`) and the default of the identifier case rules |
| `rule.group.case` / `case::keyword` `.disable: true` | keeps keywords as written |
| keyword case rules (`entity_004`, …) `.case`, `.disable` | per keyword (see `formatting.md`) |
| `blank_line` rules `.style`, `whitespace_200`, `pragma_400`–`pragma_403` | applied by the formatter |
| alignment rules `.disable` and group options | applied by the formatter per kind of alignment (see `formatting.md`) |
| rule options such as `action`, `parenthesis`, `case`, `case_exceptions`, `prefix_exceptions`, `suffix_exceptions`, `regex`, `prefixes`, `suffixes`, `exceptions`, `names`, `consecutive`, `method`, `clock`, `magnitude`, `units`, `keywords`, `standard` and the `block_comment` options | supported by the rules that define them |
| `linesep` | supported (`"\n"` or `"\r\n"`); without it, the input's line ending is kept |
| `file_list` | supported: paths and glob patterns (`*`, `?`, `**`, environment variables), relative to the working directory, each optionally with its own `rule` block; the files are checked in addition to those on the command line, and a pattern that matches nothing is an error, as in VSG |
| `file_rules` | supported, as a mapping or a list; keys are paths or glob patterns (`*`, `?`, `**`); a relative pattern matches any path ending with it |
| `pragma.patterns` | supported (`open`, `close` regular expressions) |
| `indent.tokens` | supported for construct-level indentation (see `formatting.md`); other settings are reported |
| `local_rules` | supported by running VSG (see below); settings of local rules are passed to VSG |
| `rule.length_001.error_length` | vsg-rs extension: lines longer than this are reported with severity `error` (formatting still uses `length`) |
| unknown rule ids | warning; the settings are ignored |

Settings of formatter-owned rules that vsg-rs does not map individually (for example most
whitespace and indentation rules) are accepted without warnings. Their behaviour comes from the
formatter's policy.

### Discovery

VSG only reads configuration passed with `-c`. vsg-rs also accepts `-c/--config` (repeatable;
later files override earlier ones). Without it, vsg-rs looks for `vsg-rs.yaml`, `.vsg-rs.yaml`,
`vsg-rs.json` or `.vsg-rs.json` in the file's directory and its parents, which is how editor
integrations work without extra arguments. An existing VSG configuration can be used by passing it
with `-c` or by copying it to one of these names.

## Command line

`vsg-rs` accepts VSG 3.35's arguments with the same meaning: `-f`, positional file names, `-c`,
`--fix`, `-fp`, `-j`, `-js`, `-of {vsg,syntastic,summary}`, `-b`, `-oc`, `-rc`, `--style
{indent_only,jcl}`, `-v`, `-ap`, `--fix_only`, `--stdin`, `--force_fix`, `--quality_report`,
`-p` and `--debug`. The console report, JSON, JUnit and GitLab code-quality files have VSG's
layout.

| Argument | Difference |
|---|---|
| `-fp`, `-ap` | accepted, no effect: there is one phase |
| `-lr` | runs the local rules with an installed VSG (see below); `local_rules` in the configuration wins, as in VSG |
| `--force_fix` | accepted, no effect: files with syntax errors are never changed |
| `--stdin --fix` | prints the fixed source on stdout and the report on stderr (VSG 3.35 fails); exit code 0 when code is printed |
| `-v` | prints vsg-rs's version |
| directories | reported as `source_file_001` (as in VSG); with `--recursive`, their VHDL files are checked |

vsg-rs additions: `--unsafe_fixes`, `--diff`, `--range START:END`, `--stdin_filename PATH`,
`--sarif FILE`, `--list_rules`, `--statistics`, `--recursive`, and configuration discovery (`vsg-rs.yaml` / `.vsg-rs.yaml` /
`.json` next to the input) when `-c` is not given.

### Exit codes

As in VSG: `0` when no error-severity violations were found, `1` otherwise (also for files that
could not be processed, missing files and invalid arguments).

## Intentional differences

* **One phase.** VSG stops reporting at the first phase with violations and fixes phase by phase,
  sometimes needing several `--fix` runs. vsg-rs reports all violations at once. `--fix`
  applies the fixes of a snapshot in one transaction; fixes that overlap an applied one are
  resolved again on the new text (a bounded number of rounds), and the result is formatted once.
  Running it again changes nothing.
* **Formatting is not a set of fixes.** Whitespace, indentation, alignment, blank lines, keyword
  case and line length are handled by one formatter with one canonical layout. Each change the
  formatter would make is reported under the VSG rule that reports that kind of change
  (keyword case exactly; the others through a table learned from VSG's reports on real code,
  `src/layout_rules.json`). Changes without a known rule are reported as `format`, and where the
  default layouts of VSG and vsg-rs differ, the reports differ too.
* **`length_001` is fixable.** VSG never shortens lines. vsg-rs folds them. `length_001` reports only
  what remains, and says whether `--fix` would fold it.
* **Which fixes `--fix` applies.** The same as VSG: the fixes of rules VSG fixes by default
  (its per-rule `fixable` default, or `fixable` from the configuration). Fixes of rules VSG does
  not fix by default (for example `port_023`, adding a port mode, or `signal_007`, removing a
  default value), and removing a statement label that is referenced elsewhere (which would
  break the code), are applied only with `--unsafe_fixes`.
* **Identifier case consistency.** The consistency rules (`signal_014`, …) target the spelling
  the declaration's case rule requires, so declaration and uses are fixed in the same run. Set
  `spelling: declaration` on a consistency rule to compare with the declaration as written, as
  VSG does.
* **Line numbers.** VSG sometimes reports a port or statement on the blank line above it;
  vsg-rs reports the line of the construct.
* **Syntax errors.** VSG may try to fix files it cannot fully parse. vsg-rs leaves such files
  untouched and reports the first syntax error.
* **Output verification.** Every fixed or formatted file is re-parsed and compared token by token
  (and comment by comment) with the input before it is written.
* **`if_002`.** A condition that is a single name ending in a parenthesized part
  (`rising_edge(clk)`, `valid(i)`) counts as enclosed, which matches what VSG 3.35 does in
  practice.
* **`-- vsg_off [rules]` / `-- vsg_on [rules]`** suppress the listed rules (all rules without a
  list) between the comments, as in VSG. A bare `-- vsg_off` also switches formatting off (see
  `formatting.md`); `-- vsg-rs: fmt off` switches off only formatting.
* **Line width** is measured in characters (Unicode scalar values for UTF-8 files), not bytes.

## Local rules

VSG's local rules are Python classes for VSG's own rule engine, so vsg-rs cannot run them
itself. With `-lr DIR` or `local_rules: DIR`, vsg-rs runs the installed VSG: `vsg` on the path,
or the command in the environment variable `VSG_RS_VSG` (split at spaces, for example
`VSG_RS_VSG="uvx --from vsg==3.35.0 vsg"`). VSG gets the same configuration files plus one that
disables every built-in rule, so that only the local rules run.

* Without `--fix`, VSG checks the inputs and its findings are added to the report.
* With `--fix`, VSG first fixes copies of the inputs with the local rules; vsg-rs then fixes
  and formats the copies' contents and writes the originals (or prints the diff). VSG then
  checks the result, and what the local rules still report is added. The copies have other
  paths than the originals, so `file_rules` patterns in the configuration do not match them in
  VSG during fixing. With `--fix --range`, local rules only report.
* If VSG cannot be started or fails, the error is printed and the exit code is 1.
* VSG 3.35 crashes when the configuration sets `severity` for a local rule.

## Measuring compatibility

`scripts/compare_vsg.py FILE...` runs VSG 3.35 and vsg-rs on the same files and prints the
findings per rule that both, only VSG, or only vsg-rs report.

## Language coverage

Measured over a 3,000-file corpus: 3 files (0.1%) do not parse and are reported and left
unchanged. Two are non-UTF-8 charset fixtures, one uses PSL written as code.

| Construct | Parses |
|---|---|
| VHDL-2008 external names, `case?`, contexts, generic package instantiation | yes |
| VHDL-2019 conditional analysis (`` `if ``), generic subprograms, interface packages | yes |
| VHDL-2019 mode views (`view v of r`, `port (x : view v)`) | no |
| VHDL-2019 conditional expressions in a declaration (`:= if c then a else b`) | no |
| PSL in comments (`-- psl assert ...`) | yes (comments are never changed) |
| PSL as code (`default clock is ...`, `assert always (a -> b) @clk`) | no |

The gaps are in the parser (`vhdl_syntax`), not in the rules; a file that does not parse is
never modified.

## The configured-project benchmark

VUnit measures how the rules are written; a heavily configured project measures how well the
configuration is honoured, which is a different thing. [open-logic](https://github.com/open-logic/open-logic)
configures **825 rules** across 4,500 lines, and the weekly job now compares against it too.

Two findings from the first run are worth recording:

* **A configuration written for VSG 3.2x does not load in 3.35 at all.** open-logic pins
  `vsg==3.27`; VSG 3.35 rejects their file because it names nine rules that were since renamed or
  merged (`generic_017`, `ieee_500` and `port_018` all became `type_mark_500`, and so on).
  `scripts/migrate_vsg_config.py` rewrites those names, and is useful to any project upgrading.
  Once migrated, **VSG 3.35 itself reports 1,986 violations** on code that VSG 3.27 passes, so
  the version step is a real event independent of vsg-rs.
* **vsg-rs reported 10,134 against those 1,986**, and the difference was almost entirely
  configuration that vsg-rs did not honour. Implementing `compact_alignment` and the `:=`
  alignment of interface lists brought it to 5,824.

What still differs, in order: indentation (`indent.tokens` settings vsg-rs does not implement),
`port_map_300` and `generic_map_300`, `procedure_509`, `whitespace_008` and `block_comment_003`.
The gap is a number that should keep falling; it is not a reason to prefer one tool over the
other on a project whose configuration both tools honour.

## Known gaps

See `rule-status.md` for rule coverage. Not supported: `indent.tokens` settings that are not about construct-level indentation (see `formatting.md`).
The consistency rules check other files only when they are passed in the same run.

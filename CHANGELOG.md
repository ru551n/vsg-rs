# Changelog

## Unreleased

* `--statistics` prints the violations per rule over all inputs, with how many files each
  affects and whether `--fix` fixes it.
* `comment_004` (`number_of_spaces`) sets the spaces before a trailing comment.
* `--range` works with files, not only with `--stdin`.
* A GitLab CI recipe (`docs/gitlab-ci.md`): code-quality report, JUnit and `--statistics`.
* More layout violations report the VSG rule id instead of `format`: the learned table covers
  410 kinds of token-pair change, up from 301. The same lines are reported, attributed more
  precisely (on 150 VUnit files, `format` findings fall from 15118 to 11851).
* A weekly job measures agreement with VSG 3.35 over VUnit's VHDL and writes the per-rule table
  to the job summary (`scripts/compare_vsg.py --markdown`).
* `vsg_rs: reflow_comments` re-wraps comment paragraphs to the line width (off by default; VSG
  has no such rule). Structured comments, directives and formatter-off regions are left alone.
* `FormatConfig` is `#[non_exhaustive]`: struct literals of it no longer compile outside the
  crate (build one from `FormatConfig::default()` instead), and adding an option is no longer a
  breaking change.
* The fuzzer also generates configurations, so formatting has to be stable under any settings,
  not only the default ones.

## 0.10.0

Breaking for users of the Rust library; the command line, the configuration and the GitHub
Action are unchanged.

* Internal: the rule catalog lists only the 191 rules the formatter does not own and takes the
  ids from VSG's bundled defaults; duplicated helpers in the rule modules, two identical check
  entry points and a hand-rolled recursive merge are gone; `Doc::Choice` has two alternatives
  instead of a list. No change in behaviour (identical findings on a 293-file corpus).
* The library no longer exports `rules::check_and_format`, `rules::implemented` and
  `fix_edits`, which had no users; `rules::check_for_fixes` and `rules::check_canonical` are one
  `check_unformatted`.
* Wheels are tested on Python 3.10 and 3.14 instead of all five versions; the wheel is the same
  bytes for every version.
* CI checks licences, duplicate crates and advisories (`cargo deny`), coverage, unused
  dependencies, spelling and documentation examples, compares the benchmarks with the base
  branch and reports public API changes; the formatter is fuzzed nightly.

## 0.9.6

* The GitHub Action still posts new suggestions when it may not resolve earlier ones, and warns
  that resolving review threads needs `contents: write`. The documentation's workflow
  examples grant it.

## 0.9.5

* The GitHub Action resolves its suggestion threads that no longer apply on each run (and
  reopens one when the same suggestion applies again), so only open problems stay expanded.
* The documentation's workflow examples report without code scanning, which stays optional.
* The alignment of `:` in generic, port and parameter lists (`entity_017`, `component_017`,
  `procedure_410`) and of `=>` in maps (`instantiation_010`) can be disabled, and
  `group: alignment: disable` now turns it off too.

## 0.9.4

Less noise on pull requests.

* SARIF reports combine layout findings on adjacent lines into one `format` result per block,
  naming the VSG rules involved; rule violations stay one result each.
* The GitHub Action posts layout changes as suggested changes in one review (`layout:
  suggestions`, the default) instead of code scanning alerts, keeps one summary comment per
  pull request up to date (`pr-comment`), and annotates only the lines a pull request adds or
  changes (`annotations: changed`).

## 0.9.3

* SARIF reports describe each rule (name, description and a link to VSG's documentation), so
  code scanning alerts and pull request comments show which rule was violated.
* The README explains how to use the GitHub Action.

## 0.9.2

* A GitHub Action (`uses: ru551n/vsg-rs@v0.9.2`): downloads the release binary, runs vsg-rs,
  shows findings as annotations and in the job summary, and optionally uploads them to code
  scanning. See `docs/github-action.md`.
* SARIF reports use paths relative to the working directory with `/` separators (as code
  scanning expects), and `file://` URIs for files outside it.

## 0.9.1

* The standalone binary archives on GitHub releases get build provenance attestations too
  (0.9.0 attested only the Python packages).

## 0.9.0

* VSG local rules (`-lr DIR`, `local_rules: DIR`) are supported: vsg-rs runs them with an
  installed VSG (`vsg`, or the command in `VSG_RS_VSG`) with the built-in rules disabled, adds
  their findings to its report and, with `--fix`, applies their fixes before its own.
* Settings of rules vsg-rs does not know no longer cause warnings when local rules are used.
* `-oc` includes `local_rules`.
* Release artifacts get build provenance attestations now that the repository is public.

## 0.8.0

Performance.

* Several files are checked and fixed in parallel worker processes instead of threads. The
  parser's global token interner made threads contend on every token: VUnit's 222 files now
  take 0.6 s instead of 1.7 s, with 3.7 s of CPU time instead of 30 s. The cross-file
  declaration index is built by the workers too.
* `--debug` prints how long the declaration index took and how many processes were used.

## 0.7.0

Command line and distribution.

* `file_list` in the configuration is supported as in VSG: paths and glob patterns (with
  environment variables), each optionally with its own `rule` block, checked together with the
  files on the command line.
* `--recursive` checks the VHDL files in directories given as inputs.
* macOS wheels (arm64 and x86_64) on PyPI.
* pre-commit hooks `vsg-rs` and `vsg-rs-fix` (`.pre-commit-hooks.yaml`).
* A migration guide from VSG (`docs/migrating-from-vsg.md`), with CI and pre-commit setups.

## 0.6.0

Formatter options.

* `number_of_spaces` of VSG's 175 spacing rules sets the spaces between the tokens each rule is
  about, within its construct; `>=N` keeps wider source spacing. The token pairs are generated
  from VSG's rule documentation (`scripts/gen_spacing_rules.py`).
* `port_007` to `port_009` `spaces_before` / `spaces_after` set the spaces around port modes.
* `action: same_line` in `generic_010`, `port_014`, `generic_map_004` and `port_map_004` keeps
  the closing parenthesis on the last element's line.
* `align_left: 'yes'` with `align_paren: 'no'` in `concurrent_003` or `sequential_004` indents
  continuation lines one level instead of aligning them.
* The randomized fix test and the corpus example (`CONFIG=file`) cover these options.

## 0.5.0

Correctness.

* Fixes that delete text no longer join neighbouring words or start a comment
  (`if(a)then` with `if_002` `parenthesis: remove` produced `ifa`).
* Consistency rules skip names that the file declares with different kinds of declaration
  (for example a signal and a variable), where only name resolution could tell which one a use
  refers to.
* A new randomized test fixes files twice under random configurations (case, actions,
  disabled groups, alignment, `indent.tokens`, unsafe fixes) and requires no internal error
  and no change in the second run; it passes on the real-world corpus.
* `reserved_001` reports declarations only, and `type_mark_500` skips subprogram parameters and
  protected types (with the comparison against VSG: 12 findings that only vsg-rs reports, down
  from 49).

## 0.4.0

Measurable VSG compatibility.

* Layout findings are reported under the VSG rule that reports each kind of change (keyword
  case exactly; indentation, spacing, line breaks, blank lines and comment columns through a
  table learned from VSG 3.35's reports, `src/layout_rules.json`). Unknown changes stay `format`.
* `scripts/compare_vsg.py` compares findings per rule with VSG; `scripts/learn_layout_rules.py`
  relearns the layout table.
* Consistency rules accept `spelling: declaration` to compare with the declaration as written.
* Closer to VSG: `if_002` accepts names with any parenthesized part, `type_mark_500` skips
  subprogram parameters and protected types, `reserved_001` checks declarations only and knows
  the VHDL-AMS words, `process_018` and `loop_statement_007` also report unlabelled statements,
  and the texts of `component_021` and `block_comment_002` match VSG.

## 0.3.0

* The command line is VSG's (one phase) with `--unsafe_fixes`, `--diff`, `--range`,
  `--stdin_filename`, `--sarif` and `--list_rules`.
* `--fix` applies VSG's default fix set plus formatting.
* `indent.tokens`, contextual keyword case, cross-file consistency, VSG solution texts.
* Standalone binaries for Linux, Windows and macOS.

## 0.2.0

* 192 VSG rules with fixes, blank-line and alignment policies, per-keyword case, smart tabs,
  signature folding, VHDL-2019 tool directives.

## 0.1.0

* First release: formatter with line folding, structural rules, VSG configuration.

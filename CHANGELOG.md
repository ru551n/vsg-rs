# Changelog

Every release says which version of VSG it targets: the rule set, the configuration and the
reports are that version's. `vsg-rs --version` prints the same thing.

## Unreleased

**Targets VSG 3.35.**

* **The lint layer reports definite errors by default.** *This changes what a default run
  reports.* Every rule now states how sure it is, and only those that can show a program cannot
  do what it says run unless you ask for more: 53 of 71 today. A finding from a default run is
  something to correct rather than something to weigh up, and an empty report means vsg-rs
  proved nothing rather than that it merely stayed quiet.

    Thirteen rules moved to **off by default**, each reporting something that is legal VHDL and
    may well be meant: `lint_001`, `lint_002`, `lint_004`, `lint_005`, `lint_006`, `lint_600`,
    `lint_601`, `lint_710`, `lint_711`, `lint_712`, `lint_720`, `lint_730` and `lint_750`. They
    join `lint_602`, `lint_603`, `lint_700`, `lint_713` and `lint_760`, which were already off.
    Two drivers on a resolved type are what a resolution function is for; an unused declaration
    is legal; a state nothing enters may be reserved.

    **To get the old behaviour**, ask for the classes:

    ```yaml
    rule:
      group:
        advisory:
          disable: false
        experimental:
          disable: false
    ```

    Or name a rule, as before. Nothing was removed and no rule id changed. An enabled rule still
    reports at `error` severity and still fails a build.

    The class also decides how a finding is filed: only a definite error is a SonarQube **bug**,
    so an advisory rule you switched on arrives as a code smell rather than claiming the design
    is broken. `--list_rules` prints each rule's class and whether a default run uses it, and the
    [rule reference](https://vsg-rs.readthedocs.io/en/latest/rule-reference/) is grouped by the
    same thing — all of them read one registry, so they cannot drift apart.

* **A language server.** `vsg-rs lsp` serves diagnostics, quick fixes and formatting over LSP,
  from the same library the command line uses: the same parser, formatter, analysis and
  configuration, so an editor and CI cannot disagree. It is deliberately narrow and does not
  advertise completion, hover, definition, references, rename or symbols — those belong to a VHDL
  language server such as `vhdl_ls`, which it is meant to run beside. Diagnostics carry their
  related locations, quick fixes come from the fix a finding already holds, and `source.fixAll`
  applies what `--fix` would. See [the language server](docs/lsp.md).
* **A VS Code extension**, in `editors/vscode`, which launches the server and nothing else. It
  ships the server for its platform, built by the same release, so there is no version to keep in
  step. Rules and layout still come from the project's own configuration file rather than from
  editor settings.
* **Analysis runs on buffers, not only files.** `--stdin --check lint` analyses what it is given,
  in the context of the project its path belongs to, which is what lets an unsaved file be checked
  at all. The analysis layer moved into the library (`vsg_rs::analysis`), so anything that is not
  the command line can use it.
* **Findings carry their other locations as data.** A multiply driven signal points at each
  driver and a combinational loop at each signal on it, rather than listing line numbers inside a
  sentence. The console shows them under the finding, SARIF carries them as `relatedLocations`,
  and SARIF now also carries safe fixes as `fixes`.
* **`--check lint` is about five times faster.** The style layer no longer runs when only lint was
  asked for, the cross-file index is built once for whichever layers need it, and the lint layer
  runs in the same worker processes the style layer uses. On VUnit's 863 files: 3.1s to 0.6s.
* `lint_700` (clock domain crossings) is now **off by default**. It infers which signal is a
  clock rather than deriving it, so it cannot point at the evidence the other rules can; enable it
  with `rule: lint_700: disable: false`.
* `lint_006` described `UnassociatedContext` as "a context declared but never used". It reports a
  context clause that is not attached to any design unit, which is a different thing.
* `--sonarqube FILE` writes SonarQube's generic issue JSON. SonarQube reads SARIF too, but files
  every SARIF issue as a vulnerability; this format carries the type, so the lint layer arrives as
  a bug and style as a code smell. Severity separates what needs a person from what does not: a
  finding `--fix` repairs is `INFO`, a lint finding is `CRITICAL`. Jenkins needs nothing new —
  Warnings-NG parses the SARIF file.
* `lint_740` reports a vector assigned to one of a different width — legal VHDL that fails only
  when the design elaborates. It measures only whole objects with literal ranges, so what it
  reports is certain.
* `lint_700` reports an unsynchronised clock domain crossing: a register from one clock used in
  logic on another. A plain capture into a flop is read as a synchroniser's first stage, and
  `vsg_rs: synchronizers` names the entities a crossing may safely pass through.
* `lint_710` and `lint_711` read enumerated state machines out of the source and report a state
  nothing can enter and a state nothing can leave — the two checks a netlist is usually thought
  necessary for.
* `lint_720` reports a combinational loop: a signal that depends on itself with no register in
  the way. The cycle is found in the source, not in a netlist.
* `lint_730` reports a signal that something reads but nothing drives — no assignment, and no
  instance output. It is the first check built on a design-wide view: the run now reads every
  input for its entities and port modes, so a port map can be read as drivers and readers.
* Every finding carries its layer (`style`, `layout` or `lint`), derived from the rule id so it
  cannot disagree with what produced it. `--statistics` shows it per rule and totals per layer,
  and `--fail_on style,layout,lint` chooses which layers make the run fail while the rest are
  still reported — so a team can gate CI on the lint layer while style only informs.
* `--explain RULE` says what a rule checks, which layer runs it, whether it is fixed, and links
  to VSG's documentation for VSG's own rules.
* A second layer of rules (`docs/lint.md`). The root command stays VSG's — same arguments, same
  reports, byte-identical output — and `vsg-rs lint ...` runs rules that need names resolved,
  through `vhdl_lang`. `--check style,lint` runs both in one pass.
* 58 lint rules: sensitivity lists, unused declarations, and the name, type, subprogram and
  association diagnostics `vhdl_lang` produces, each with an id and a description in
  `--list_rules`.
* Three checks of our own: `lint_600` infers a latch when a combinational process does not
  assign a signal on every path (variables included, when one is read before it is written),
  `lint_601` reports a signal driven by more than one concurrent statement, and `lint_602` and
  `lint_603` check that a registered signal carries a suffix or prefix (both off by default,
  with plain, glob or regular-expression patterns).
* `ieee` and `std` are embedded in the binary, so the lint layer resolves them with no simulator
  and no configuration. `NOTICE` credits the IEEE P1076 WG and rust_hdl sources.
* Rules that need cross-file resolution wait for a `vhdl_ls.toml`, because unresolved names make
  them meaningless. A lint run without one says how many rules did not run, every time.
* Testbench and RTL code can carry different lint rules: `vsg_rs: testbench_files` (globs) or
  `vsg_rs: testbench_libraries` (from `vhdl_ls.toml`) name the testbenches, and
  `vsg_rs: testbench: rule:` / `vsg_rs: rtl: rule:` hold a rule block each. Without either, a
  file is classified by its own shape, and `--debug` says which files and why.
* `--lint_configuration` (`-lc`) takes configuration files applied to the lint layer only.
* `compact_alignment` is implemented. With it (VSG's default) an aligned column is the narrowest
  that fits; without it a group that already agrees on a wider column keeps it.
* The `:=` of generic and port clauses is aligned after the type, as VSG does (`entity_018`); it
  was previously collapsed to one space.
* `scripts/migrate_vsg_config.py` rewrites a VSG 3.2x configuration to the rule names 3.35 uses.
* The documentation is published at <https://vsg-rs.readthedocs.io/>. It covers what vsg-rs adds
  on top of VSG; the rules, their options and the configuration file are VSG's and are linked to
  rather than repeated, so the two cannot drift apart.
* The weekly compatibility job gains open-logic (825 configured rules) as a second corpus and a
  third comparison against whatever VSG released most recently.

## 0.11.0

**Targets VSG 3.35.**

* Waivers (`docs/waivers.md`): `--waivers FILE` accepts the violations a project has decided to
  live with, listed by rule, file glob, lines and reason. `--generate_waivers FILE` writes a
  file covering everything found now, so a rule set can be adopted on existing code in one
  command, and `--show_waived` lists what was waived instead of only counting it. Waived
  violations never affect the exit code.
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

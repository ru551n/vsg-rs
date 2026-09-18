# From formatter to full linter — the TODO

What vsg-rs would need to be a linter people pick over a commercial tool, rather than a style
checker that happens to be free. Ordered by value per effort. `[x]` is already in the tool.

Sources for "what a linter is expected to have": Verible (autofix in place or as a patch, waiver
files, upward config search, language server), svlint (TOML rule config), Sigasi (262 numbered
rules, quick fixes at file, library or project scope, a `verify` CLI with JSON, SonarQube-generic
and Warnings-NG output, inline `@suppress`), Linty (SonarQube quality gates, netlist checks,
published DO-254 and CNES mappings), and the standard RTL lint checklist (latch inference,
incomplete sensitivity lists, multiple drivers, width mismatches, unread logic). Details and
citations in `comparison.md`.

## 1. Semantic analysis — the one gap that matters

Everything below the style layer needs name resolution. The cheap route exists: **`vhdl_lang`**,
from the same project as our parser (`vhdl_syntax`), MPL-2.0, already emits ~58 semantic
diagnostics and ships `lint/sensitivity_list.rs` and `lint/dead_code.rs`. It keeps its own AST,
so this is a second parse in a second phase, not a change to the formatter.

- [ ] **Decide the rule-id namespace.** VSG compatibility means no invented `signal_0xx` ids.
      Proposal: a `semantic_*` namespace, every rule disabled by default, switched on with one
      `group: semantic: enable: true`. Blocks everything else in this section.
- [ ] **Wire `vhdl_lang` in as an optional phase** behind a feature flag, so the default binary
      stays one fast parse. Map its `ErrorCode`s to vsg-rs findings and severities.
      **The engine is a Rust library, never a subprocess.** GHDL's `-W` flags cover much of this
      list, and wrapping them would be quicker, but it would make a style check depend on a
      simulator being installed and on parsing another tool's prose. `vhdl_lang` is a crate from
      the same project as the parser, so it stays one binary with no external tool.
- [ ] **Incomplete and superfluous sensitivity lists** (`MissingInSensitivityList`,
      `SuperfluousInSensitivityList`, `DisallowedInSensitivityList`). Top of every RTL lint
      checklist; already implemented upstream.
- [ ] **Unused and dead code** (`Unused`, plus `lint/dead_code.rs`): unused declarations, ports,
      generics, signals never read.
- [ ] **Signals never written, and read before assignment.** Needs dataflow on top of resolution;
      upstream does not have it.
- [ ] **Type and width checking** (`TypeMismatch`, `DimensionMismatch`): width mismatches in
      assignments and port maps, comparison of vectors of different lengths.
- [ ] **Port and generic maps against the real entity** (`Unassociated`, `AlreadyAssociated`,
      `InvalidFormal`): missing, extra, duplicated or misordered associations, and component
      versus entity mismatch. Needs the library model below.
- [ ] **Case completeness** against the type's value set, plus overlapping and out-of-range
      choices.
- [ ] **Latch inference** (an `if` without `else`, a `case` without `others`, a partially assigned
      vector in a combinational process). The highest-value RTL check that does *not* need a
      netlist: a per-process assignment analysis covers the common cases.
- [ ] **Multiple drivers** on one signal across processes and concurrent assignments.
- [ ] **Clock and reset heuristics**: a clock not used as a clock, mixed edges, mixed sync/async
      reset style, a register without a reset. Source level, the way Sigasi does them.

### How it joins the style layer

One report, three producers. `rules::Violation` is the join point and `Owner::Semantic` already
exists, so the report, SARIF, JUnit, `--statistics`, severities, `file_rules`, waivers and exit
codes stay one code path. Four rules keep the layers from contaminating each other:

1. **Semantic findings never carry a fix** (`fix: None`, always). The fixer's safety net is the
   verifier: output must re-parse with identical tokens and comments. A semantic fix changes
   meaning by definition, so it cannot pass that check and must not try.
2. **Semantic runs after fixing, on the final bytes.** A `--fix` run rewrites the file, so
   positions from a pre-fix analysis would point at the wrong lines. Order: style check or fix,
   write, then analyse what is now on disk.
3. **A different unit of work, so a different phase.** Style is per file and parallel, which is
   what the worker processes are for; semantic analysis needs the whole library at once. It runs
   once per invocation in the parent, after the workers report — so the fast path never touches
   the semantic engine.
4. **Degrade, never fail.** Without a library map or with analysis errors, report the style
   findings anyway and warn, the contract `local_rules` already has.

Configuration stays one mental model: `semantic_*` ids live in the same `rule:` map, so `disable`,
`severity` and `file_rules` work on them unchanged, with `group: semantic` to switch the set.

- [ ] **Add a `kind` to findings** (layout / style / semantic). Needed for `--statistics`
      grouping, SARIF tags, and CI gating such as "fail on semantic errors, warn on style" —
      without it no team can adopt the semantic set incrementally.
- [ ] **Keep it one binary.** Shared configuration and one report are worth more than a clean
      split; the phase separation above is the isolation.

## 2. Project model

Everything in §1 beyond a single file needs to know what a library is.

- [ ] **Read `vhdl_ls.toml`** for library mapping (read only; never write one into a repository).
- [ ] **`--top` awareness**, so rules can treat the top level differently (Linty's
      `inout`-only-at-top rule is the canonical example).
- [ ] **Testbench versus RTL scoping**: path patterns marking testbench files, and a second
      severity per rule for RTL, as both commercial tools have.
- [ ] **Mark the current cross-file consistency rules as heuristics** in the report once real
      binding exists, since they match by name rather than by binding.

## 3. Waivers and triage — the cheapest way to be usable in CI

- [x] Inline `-- vsg_off` / `-- vsg-rs: fmt off` regions.
- [ ] **Inline waiver with a reason**: `-- vsg-rs: waive signal_008 "generated code"`, reported as
      waived rather than silently dropped.
- [ ] **Waiver files** (Verible's model): rule, path, line and reason, plus
      **`--generate-waivers`** to write the current findings out as one. That is adoption on a
      legacy codebase in a single command.
- [ ] **`--waived` reporting**: count and list what was waived, so waivers cannot rot unnoticed.
- [ ] *(Previously declined: baseline/ratchet mode.)* The waiver file covers most of it; say the
      word if the stateful version is wanted after all.

## 4. Reporting and integrations

- [x] SARIF, JUnit, GitLab code quality, syntastic, JSON, `--statistics`, the GitHub Action and
      pre-commit hooks.
- [ ] **SonarQube generic issue JSON** — the format both Sigasi and Linty feed; it opens every
      SonarQube shop without them needing a plugin.
- [ ] **Warnings NG XML** for Jenkins, the other format Sigasi's CLI emits.
- [ ] **A published standards mapping** (DO-254, STARC, CNES), rule by rule, as a documentation
      table. Both commercial tools sell this; no free VHDL tool publishes one.
- [ ] **A Docker image** for pipelines that cannot install a binary.
- [ ] **`--fix --diff` as a review artifact**: emit the patch for any CI, not only as GitHub
      suggestions.

## 5. Rule authoring

- [x] VSG `local_rules` (runs an installed VSG). The one place vsg-rs calls an external tool, and
      it exists for VSG compatibility; it is opt-in and nothing else may follow that pattern. The
      native rules below are what removes the need for it.
- [ ] **Native custom rules without VSG**: a declarative query over the syntax tree (node kind,
      token pattern, optional name condition) written in YAML, so a team can add "no
      `std_logic_arith`", "entity name must match the file name" or "no `variable` in a clocked
      process" without writing Rust. After autofix this is the most asked-for linter feature, and
      nothing in the VHDL world offers it for free.
- [ ] **`--explain RULE`**: what the rule checks, why, and whether it is fixable (Clippy's and
      Ruff's model).

## 6. Performance and developer experience

- [x] Multi-process checking, worker chunking, `--range`, `--stdin`.
- [ ] **Result caching** keyed on the file hash and the configuration hash, so an unchanged file
      is not re-checked. The obvious win for pre-commit and large repositories.
- [ ] **`--watch`** for local iteration.
- [ ] **Upward configuration search** (Verible's `.rules.verible_lint` behaviour): find `vsg.yaml`
      by walking up from each file, so a subdirectory can refine the style.
- [ ] *(Declined, and staying declined: a language server — `vhdl_ls` already is one; style
      presets; a configuration generator.)*

## 7. Distribution

Being reachable matters as much as being good; the aggregators are where VHDL users already are.

- [ ] **A TerosHDL backend.** Its VHDL style linter is VSG only today, and its formatter list is
      VSG plus its own. vsg-rs is a drop-in for both, and faster.
- [ ] **Emacs `vhdl-ext`** wires up `vhdl_ls`, GHDL and VSG; adding vsg-rs is a small patch.
- [ ] **Keep the Bazel path in mind**: `hw-bzl/rules_vsg` exists for VSG, so a `rules_vsg_rs` or a
      drop-in flag is cheap to offer.

## 8. Language coverage

- [ ] **VHDL-2019 mode views** and **conditional expressions in declarations** — currently
      unparsed (`compatibility.md`). Parser work, upstream.
- [ ] **PSL written as code** — unparsed. Such files are reported and left untouched, which is
      safe, but a PSL-heavy project cannot use vsg-rs at all.
- [ ] **Keep measuring**: the weekly compatibility job reports agreement with VSG; add the
      parse-failure rate to the same summary.

## The order I would actually do it in

1. Waiver files and `--generate-waivers` (§3) — unblocks adoption on existing codebases.
2. The rule-id namespace, then the `vhdl_lang` phase (§1) — everything semantic waits on it.
3. Sensitivity lists, unused and dead code, latch inference (§1) — the three checks every RTL
   lint checklist opens with.
4. `vhdl_ls.toml` library mapping (§2) — turns the rest of §1 on.
5. Declarative custom rules (§5) — the differentiator no free VHDL tool has.
6. SonarQube and Warnings-NG output, caching, upward config search — small and mechanical.

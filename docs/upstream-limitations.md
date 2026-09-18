# VSG Documented Known Limitations

Source: VSG's own docs, `Overview` section ("Known Limitations"), fetched from
`docs/overview.rst` (mirrored in `README.rst`) in jeremiah-c-leary/vhdl-style-guide.
As of the docs snapshot examined, VSG's own stated known limitations are
exactly two bullet points — the project does not maintain a longer/separate
"known issues" page beyond this. Everything below is paraphrased in my own
words; no upstream prose is copied. I supplement each with a corroborating
"undocumented but observed" limitation surfaced by the bug/issue research in
`upstream-bugs.md`, since the two-line official list understates the tool's
real edge, and note plausibility of a CST-based single-parse fix for each.

---

## Status in vsg-rs

| Limitation | vsg-rs |
|---|---|
| L1 PSL | Blocked by the frontend: `vhdl_syntax` does not parse PSL written as code (`default clock is ...`, `assert always (a -> b) @rising_edge(clk)`); such files are reported and left untouched. PSL inside comments (`-- psl ...`) is safe, because comments are never changed. |
| L2 VHDL-2019 | Partial, measured (`compatibility.md`, "Language coverage"): conditional analysis, generic subprograms and interface packages parse; mode views (`view v of r`, `port (x : view v)`) and conditional expressions in declarations do not. Tool directives are refused for now. |
| L3 No automatic rewrapping | Fixed: `vsg-rs --fix` folds long lines (`line-folding.md`). |
| L4 No cross-file resolution | Planned for semantic rules (via `vhdl_lang`). |
| L5 No parse-error recovery | By design, files with syntax errors are never modified; the parser still recovers enough to report errors. |
| L6 No fixer idempotence | Fixed: one fix transaction; idempotence is tested on every regression reproducer and a 1,500-file corpus. |
| L7 No output safety | Fixed: every output is re-parsed and compared with the input before it is written. |

---

## L1: Minimal support for embedded PSL (Property Specification Language)
Still present in 3.35.0? Not independently black-box tested (PSL assertions embedded in VHDL comments/`--psl` are a narrow, low-frequency feature to construct a meaningful test for cheaply, and the docs still list this limitation unchanged as of the current release's `overview.rst`). Treated as still true based on the absence of any PSL-specific rule group or PSL grammar documentation anywhere in the current docs tree (no `psl_rules.rst` or similar exists in the 168-file docs listing).
Likely fixable in a CST-based single-parse formatter? Partially. PSL is a separate language embedded via VHDL comments (`-- psl ...`) or a dedicated PSL block, so "fixing" this really means: (a) recognizing PSL syntax well enough not to misparse it as an ordinary comment or code (VSG-BUG's #1411 "PSL statements wrapped in vsg_off/on cause processing error" found during this research is consistent with PSL being handled as an afterthought rather than a first-class embedded grammar), and (b) optionally offering PSL-aware formatting (indentation/alignment) as a bonus. A CST-based parser can treat a recognized PSL block as an opaque-but-well-delimited island (never letting its internal tokens confuse the surrounding VHDL scan), which is a strictly easier bar than fully formatting PSL, and removes the "processing error" failure mode even without full PSL formatting support.

## L2: The parser does not process VHDL-2019
Still present in 3.35.0? Not independently tested with a full VHDL-2019 program (constructing a minimal but genuinely 2019-only construct — e.g. the extended `interface_incomplete_type_declaration`/generic view features and unbounded external names as unconstrained arrays — and confirming a hard parser rejection was outside this pass's "cheap" budget). Inferred still true: the docs tree's grammar-adjacent rule groups (e.g. `interface_incomplete_type_declaration_rules.rst`, which is a VHDL-2008 feature, not 2019) show no trace of VHDL-2019-specific constructs (no rules for `view`/mode-view declarations, no conditional-analysis/2019-only generic-map features), and the `overview.rst` known-limitations text still states this plainly in the current snapshot.
Likely fixable in a CST-based single-parse formatter? Yes, in the sense that VHDL-2019 grammar coverage is purely a matter of investment (LRM-driven grammar completeness), not an architectural blocker — a grammar-first parser (vs. VSG's incrementally hand-extended one) makes adding a new LRM revision's productions a localized, testable change instead of another set of ad-hoc branches threaded through many classifier files (the same class of problem visible in VSG-BUG-004's resolution-function subtype gap and VSG-BUG-034's generate-statement crashes, both VHDL-2008-era gaps in a tool whose grammar predates full 2008 coverage).

---

## L3 (undocumented, observed): Rewrapping/line-splitting is never automatic
Still present in 3.35.0? Yes — confirmed directly via `vsg -rc length_001`, which reports `"fixable": false`, and corroborated by VSG's own `unfixable_rules` reference doc, which lists `length_001`/`length_002`/`length_003` under a "Lengths" category with the stated rationale that resolving line-length issues requires user judgment.
Likely fixable in a CST-based single-parse formatter? Yes — this is the standout opportunity for vsg-rs (see VSG-BUG-026). A formatter with full expression-precedence knowledge from its CST can safely choose legal break points (around operators, inside argument lists, etc.) and fit a configured width budget automatically, the way `rustfmt`/`clang-format` do for their languages. This is not a limitation inherent to formatting VHDL — it is a limitation of VSG's specific architecture (per-rule checks without a unified layout engine that reasons about width budgets holistically).

## L4 (undocumented, observed): No cross-file / cross-compilation-unit name resolution
Still present in 3.35.0? Partially — VSG-BUG-020/021/022 in `upstream-bugs.md` show that even *within a single file*, "consistent capitalization"/classification rules can misresolve names across unrelated scopes; the project's own docs and architecture (file-at-a-time, in `apply_rules.py`) give no indication of a whole-project symbol table spanning multiple files/libraries, so a type or signal declared in one file and referenced by name in another is checked only insofar as each file is self-consistent, not against its true declaration elsewhere in the design.
Likely fixable in a CST-based single-parse formatter? Only if deliberately scoped as multi-file: a *single-file* CST-based formatter (the natural "port VSG's architecture into Rust" scope) inherits the same limitation by construction, since design-unit resolution across files is a separate, larger feature (effectively a partial elaborator). It is fixable, but it's a scope decision, not a free byproduct of choosing a CST — worth flagging explicitly as either an explicit non-goal or a deliberately-scheduled later milestone for vsg-rs, so the `semantic` rule-owner bucket in `vsg-rules.md` is clearly bounded to single-file resolution unless multi-file scope is chosen.

## L5 (undocumented, observed): No formal parse-error recovery — malformed input fails unpredictably instead of degrading gracefully
Still present in 3.35.0? Yes, per this research's black-box testing: VSG-BUG-001 and VSG-BUG-006 both reproduce hard crashes (different exception types than the originally reported ones) on malformed/edge-case input in the current release, and VSG-BUG-032's large-file hang suggests at least some passes are not even bounded in time. The project does have *some* graceful parse errors (see VSG-BUG-004's original, well-formed `"Unexpected token detected... Expecting: X Found: Y"` message), so this is not absolute, but it is inconsistent — some malformed inputs get a clean diagnostic, others crash with an internal Python traceback.
Likely fixable in a CST-based single-parse formatter? Yes, and this is a core design decision worth making explicit for vsg-rs: build the parser with an explicit error-recovery strategy (error nodes that let the parser resynchronize on the next statement/semicolon rather than aborting, in the style of `rust-analyzer`'s "resilient" CST or `tree-sitter`'s error nodes) so that (a) every malformed input yields a bounded, structured diagnostic and never an internal panic, and (b) partial results (formatting for the rest of a file even when one statement fails to parse) become possible, which VSG's architecture does not support at all today.

## L6 (undocumented, observed): No idempotence guarantee for `--fix`
Still present in 3.35.0? Yes — VSG-BUG-012, VSG-BUG-013, and VSG-BUG-014 in `upstream-bugs.md` are all open, version-current reports of a fixer whose output is not a stable fixed point (running `--fix` again either re-reports the same violation, produces a different-but-still-wrong result, or keeps drifting further with each run).
Likely fixable in a CST-based single-parse formatter? Yes, and cheaply — idempotence (`fix(fix(x)) == fix(x)`) is a mechanical, cheap-to-unit-test property once the fixer derives its output purely from the CST/AST structure (not from incrementally patching the previous textual output), and per-rule idempotence tests are trivial to add to any rule-testing framework from day one.

## L7 (undocumented, observed): No output-safety guarantee — a "successful" `--fix` run can produce unparseable VHDL
Still present in 3.35.0? Yes — VSG-BUG-008 is a directly reproduced, current-version example of `--fix` reporting success while writing out syntactically invalid VHDL (a closing paren and keyword silently absorbed into a preceding comment). The `--force_fix` CLI flag (documented as "ALPHA: Apply fixes if syntax errors are detected") is itself evidence that even upstream is aware fixes can interact badly with pre-existing syntax problems, though it does not address a fix *introducing* a new syntax problem into previously-valid code, which is the more serious case observed here.
Likely fixable in a CST-based single-parse formatter? Yes, and this should be a hard architectural invariant in vsg-rs, not a best-effort feature: every `--fix` run reparses its own output before writing to disk (or writes to a scratch buffer and diffs/validates before committing), and any fix that produces unparseable output, or a semantically different token stream than intended (e.g. a token silently merged into a comment), is rejected rather than applied. This "reparse safety check" is one of the most concrete, testable design commitments this whole research effort points to.

---

### Summary count
- Documented ("Known Limitations" in VSG's own overview docs): 2 (L1 PSL, L2 VHDL-2019).
- Additional undocumented-but-observed limitations surfaced by cross-referencing the bug research: 5 (L3 line-wrapping, L4 cross-file resolution, L5 error recovery, L6 idempotence, L7 output safety).

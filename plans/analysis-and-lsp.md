# Analysis facts, LSP and VS Code — planning report

Status: **proposal, not yet approved.** No implementation has started.

Written against `main` at the merge of #37. Every claim below cites the code it was read from.

---

## 1. Current-state audit

### 1.1 What already exists

| Capability | Where | State |
|---|---|---|
| Lossless parse, one per snapshot | `src/lib.rs:38` `Parsed::new(Vec<u8>)` | in the **library**, takes bytes |
| Formatter | `src/lib.rs:290` `format`, `:295` `format_parsed`, `:379` `reindent` | in the **library**, takes bytes |
| Range formatting | `src/lib.rs:456` `format_range`, `:468` `fix_range` | library |
| Structured edits | `src/rules/mod.rs:98` `Edit {start,end,text,rank}` | byte ranges, already structured |
| Fix + safety | `src/rules/mod.rs:107` `Fix {safety, edits}`, `:91` `FixSafety::{Safe,Unsafe}` | already structured |
| Style violations | `src/rules/mod.rs:115` `Violation {rule,severity,start,end,message,fix}` | has byte range **and** fix |
| Style rule engine | `src/rules/` | library |
| Config incl. discovery | `src/config.rs`, lookup list at `:230` | library |
| Lint layer (10 native rules) | `src/{design,elaborate,fsm,combinational,clockdomain,width}.rs` | **binary crate only** |
| Front-end semantics (56 rules) | `src/lint.rs` via `vhdl_lang` 0.88 | **binary crate only** |
| Project/library map | `src/lint.rs:442` reads `vhdl_ls.toml` | **binary crate only** |
| Worker-process parallelism | `src/vsg_cli.rs` `in_workers`, `worker_chunks` | binary |
| Reports: VSG, syntastic, summary, JSON, SARIF, JUnit, Code Climate, SonarQube | `src/vsg_cli.rs` | binary |
| Waivers | `src/waivers.rs` | binary |
| Release binaries, 6 targets + `SHA256SUMS` + attestations | `.github/workflows/release.yml` | **complete** |

### 1.2 Crate boundary is the central problem

`src/main.rs:5-15` declares the whole analysis layer as **binary** modules:

```rust
mod clockdomain; mod combinational; mod design; mod elaborate; mod fsm;
mod lint; mod local_rules; mod testbench; mod vsg_cli; mod waivers; mod width;
```

`src/lib.rs` exports parsing, formatting, fixes, config and the style rules — **not** the lint
layer. Any consumer that is not the CLI (an LSP, a test, another tool) cannot reach static
analysis at all.

### 1.3 Five diagnostic representations

| Type | Where | Carries |
|---|---|---|
| `rules::Violation` | `src/rules/mod.rs:115` | byte range, severity, fix |
| `lint::Finding` | `src/lint.rs:26` | **line/column only**, no fix, no range |
| `local_rules::Finding` | `src/local_rules.rs:15` | VSG plugin findings |
| `vsg_cli::Diagnostic` | `src/vsg_cli.rs:20` | report-facing; line/column/severity/fixable |
| `lib::Diagnostic` | `src/lib.rs:65` | parse errors only (offset + message) |

**None carries related locations.** They are flattened into message text today:

```text
lint_601 | Signal 'q' is assigned by 2 concurrent statements (lines 13, 14)
lint_720 | Combinational loop: grant -> request -> grant depends on itself ...
```

SARIF emits no `relatedLocations` and no `fixes`, although `Fix`/`Edit` exist and could populate
both.

---

## 2. Architecture gaps

1. **Analysis is binary-only** (§1.2). Blocks LSP, and blocks unit-testing analysis as a library.
2. **Analysis is disk-only.** `lint::analyse(&[PathBuf])` (`src/lint.rs:470`) takes paths;
   `Project::from_config` reads from disk; every native rule does `std::fs::read(file)`
   (`src/vsg_cli.rs:1335`). There is no way to analyse a buffer.
3. **Lint is explicitly disabled for stdin**: `if lint && !args.stdin` (`src/vsg_cli.rs:1779`).
   The one in-memory path that exists deliberately skips analysis.
4. **Related locations do not exist** as data (§1.3).
5. **`lint::Finding` has no byte range**, so an LSP cannot produce a precise range without
   re-deriving it.
6. **Fixes never reach SARIF**, though they are structured.

---

## 3. LSP readiness audit

| Question | Answer | Evidence |
|---|---|---|
| Diagnostics without parsing CLI text? | **Partly.** Style yes (`rules::check_with`); lint no — binary-only. | §1.2 |
| Analysis on in-memory unsaved source? | **No.** | `src/lint.rs:470`, `src/vsg_cli.rs:1335,1779` |
| Formatting on in-memory source? | **Yes, today.** | `src/lib.rs:290` `format(Vec<u8>, &FormatConfig)` |
| Fixes structured? | **Yes, today.** | `src/rules/mod.rs:98,107` |
| Related locations structured? | **No.** | §1.3 |
| Project config loadable without CLI? | **Config yes** (`src/config.rs`); **library map no** (`src/lint.rs:442`, binary). | |
| Does analysis assume files on disk? | **Yes.** | §2.2 |

**Conclusion.** The formatter is LSP-ready now. The analysis engine is not, for two independent
reasons: crate placement and disk coupling. Both must be fixed before an LSP can exist, and both
are refactors with no behaviour change — good first PRs.

`tower-lsp-server`: 0.23.0 stable, MIT OR Apache-2.0 (allowed by `deny.toml:15`), MSRV 1.85
(project requires 1.95), 2.18M downloads, last released 2026-09-11. No blocker found. Note it
brings an async runtime into a binary that currently has none; the LSP module should be
feature-gated only if the bundled binary can still enable it — the VS Code extension ships the
same binary, so `lsp` must be on by default.

---

## 4. VS Code packaging audit

| Question | Answer |
|---|---|
| Binary targets released today? | Six, already: `x86_64`/`aarch64-unknown-linux-musl`, `x86_64`/`aarch64-pc-windows-msvc`, `x86_64`/`aarch64-apple-darwin` (`release.yml:138-143`) |
| Existing workflows? | `ci`, `release`, `compatibility`, `fuzz`, `action` |
| Asset names | `vsg-rs-$TAG-$TARGET.tar.gz` (`.zip` on Windows), plus `SHA256SUMS`, plus build attestations (`release.yml:166-176,298,302`) |
| How should packaging consume them? | Download by exact tag + target at VSIX build time, verify against `SHA256SUMS`. **No binaries committed to the extension repo.** |
| Server modes initially | `embedded` (default), `systemPath`, `userPath`. **No Docker** — the reference extension has it; the brief and the lack of demand say skip it. |
| Local development | `vsg-rs.server.mode = "userPath"`, `vsg-rs.server.path = "…/target/debug/vsg-rs"` |
| Version compatibility | Extension declares the exact server version it bundles; `Show Server Version` reports both. Independent version lines, documented. |

**Recommendation that differs from the reference extension**: publish **platform-specific VSIXs**
(`vsce package --target linux-x64` etc.) rather than one fat VSIX carrying six binaries. The
Marketplace serves the right artifact per platform, and the download stays small. The reference
extension predates this and ships Linux+Windows only; we can cover macOS from day one because the
release pipeline already builds it.

---

## 5. Target architecture — the smallest change that works

```text
                       vsg_rs (library)
   ┌───────────────┬──────────────┴─────────────┬──────────────┐
   ▼               ▼                            ▼              ▼
 Parsed        Formatter                   Analysis        Diagnostic
 (bytes)       (bytes -> bytes)            (Source -> Vec<Diagnostic>)
                                                │
                                          Fix { safety, edits }
   └───────────────┴──────────────┬─────────────┴──────────────┘
                                  ▼
                    ┌─────────────┴─────────────┐
                    ▼                           ▼
                   CLI                     vsg-rs lsp
```

Two new library concepts only:

```rust
/// What to analyse: a path, or a buffer standing in for one.
pub struct Source { pub path: PathBuf, pub text: Option<Vec<u8>> }

/// One finding, whatever produced it.
pub struct Diagnostic {
    pub rule: &'static str,
    pub severity: Severity,
    pub span: Range<usize>,          // bytes, not line/column
    pub message: String,
    pub related: Vec<Related>,       // NEW: {span, file, message}
    pub fix: Option<Fix>,            // already exists
}
```

Everything else is moving code between crates, not designing new abstractions.

---

## 6. Ordered PR plan

Repository is `vsg-rs` unless stated.

### PR 0 — Stop doing work nobody asked for
**Goal** skip the style layer when `--check lint` is given alone, and share one parse between
`elaborate::entities` and `lint_files`. **Motivation** measured waste, not speculation: the style
results are computed and cleared (`src/vsg_cli.rs:1758`), and the same bytes are parsed twice
thirty lines apart. **Behaviour** none — identical output, less time. **Tests** existing output
comparison on the corpora; a timing figure in the PR body. **Size** small. **Risk** low.
**Independent of everything else.**

### PR 1 — One diagnostic model, with related locations
**Goal** collapse `lint::Finding` and `vsg_cli::Diagnostic` onto one type carrying a byte span and
structured `related` locations. **Motivation** every consumer needs it, and it is the only change
that must happen before both SARIF enrichment and the LSP. **Files** `src/lib.rs`,
`src/rules/mod.rs`, `src/lint.rs`, `src/design.rs`, `src/vsg_cli.rs`. **Behaviour** CLI text output
unchanged except that `lint_601`/`lint_720` stop embedding line numbers in the message and render
them as related lines. **Tests** golden CLI output; a unit test per migrated rule asserting
related spans. **Docs** `docs/reports.md`. **Size** medium. **Risk** medium (touches every rule's
construction site).

### PR 2 — SARIF related locations and fixes
**Goal** emit `relatedLocations` and `fixes` now that both exist. **Depends on** PR 1.
**Size** small. **Risk** low.

### PR 3 — Move the analysis layer into the library
**Goal** `design`, `elaborate`, `fsm`, `combinational`, `clockdomain`, `width`, `lint`, `testbench`
move from binary modules to `vsg_rs::analysis::*`; the binary keeps CLI concerns only.
**Motivation** the LSP cannot exist otherwise. **Behaviour** none — pure move plus visibility
changes. **Tests** existing suite must pass unchanged; add a library-level test that runs analysis
without the binary. **Size** large (mechanical). **Risk** medium. **Note** `semver-checks` will
flag new public API; that is intended.

### PR 4 — Analyse in-memory sources
**Goal** `analysis::check(&[Source], &Config) -> Vec<Diagnostic>` where a `Source` may carry
buffer text; remove the `!args.stdin` exclusion so `--stdin --check lint` works. **Motivation**
unsaved editor buffers. **Risk** low — spiked (§10.6): `Source::inline` + `Project::update_source` is the supported
overlay path and needs no temporary file. **Tests** lint findings from stdin match findings from the same
bytes on disk. **Size** medium.

### PR 5 — `vsg-rs lsp`: minimal server
**Goal** `tower-lsp-server`; `initialize`/`shutdown`, `didOpen`/`didChange`/`didClose`,
diagnostics with related information, `textDocument/formatting` (whole-document edit).
**Explicitly advertises no** completion/hover/definition/references/rename/symbols/semantic tokens.
**Depends on** PR 1, 3, 4. **Tests** protocol tests incl. a capability-advertisement test asserting
the unsupported list is absent, and `LSP formatting == core formatter` for the same input.
**Docs** `docs/lsp.md`, `docs/editors.md`. **Size** medium. **Risk** medium.

### PR 6 — LSP code actions and fix-all
**Goal** map `Fix`/`Edit` to `CodeAction`/`WorkspaceEdit`; `source.fixAll.vsg-rs` applying
`FixSafety::Safe` only. **Depends on** PR 5. **Size** small. **Risk** low — the safety
classification already exists and is shared, so CLI/quick-fix/fix-all parity is structural.

### PR 7 — A `Context` for the lint layer
**Goal** give the lint layer the indexed, lazily-derived context the style layer already has
(`rules/mod.rs:149`): one walk, `by_kind`, and per-architecture facts (declarations with class,
type mark and literal width; processes with their clock; statements with reads/writes/words;
instances). Migrate the rules §10.4 says can become queries; leave the three that cannot.
**Motivation** §10.1-10.2. **Behaviour** none. **Tests** existing rule tests unchanged — that is
the point. **Size** large. **Risk** medium.

### PR 7b — Keep the resolved symbol table
**Goal** stop discarding the analysed `Project` (§10.5) and expose symbol/reference queries.
**Spiked (§10.6): the minimal change is to keep the `Project` and the path list alive past
`analyse()`**, then `get_source(path)` + `find_all_entity_references(&source)` yields
`(SrcPos, EntRef)` for a whole file in one call. **Size** small-to-medium. **Risk** low.
**Blocks** shadowing, call graph, reference graph.

**Half done, and the other half is bigger than this says — measured 2026-09-20 (§10.7).** The
`Project` is kept (#56), which is what made the editor fast. The query half was built and
reverted: `public_symbols` reaches only the public surface, and the rule it would feed does not
clear the bar.

### PR 8+ — New exact diagnostics
In dependency order, each small and independently valuable: unused/duplicate `use` and `library`
clauses · exact case analysis (missing/duplicate/overlapping choices, redundant `others`) ·
shadowing · subprogram call graph (unused local subprogram, recursion cycles) · component/entity
association consistency · project reference graph. Each must clear the existing bar: **zero
findings on cnn_accel, VUnit and open-logic** unless confirmed real.

**Mostly superseded — audited 2026-09-20.** The front-end rules that landed with the lint layer
already report: unused and duplicate `use`/`library` clauses (`lint_004`, `005`, `006`, `108`),
shadowing (`lint_101`), association consistency (`lint_400`–`404`), and unused subprograms
(`lint_004`). Missing and overlapping case choices are compile errors the front end names.
Redundant `others` shipped as `lint_712`, and naming every value instead of `others` as
`lint_713`. What is left of this item is the reference graph and recursion cycles, both blocked
on §10.7.

### 10.7 Spike result: what the kept symbol table can and cannot answer

Measured against `vhdl_lang` 0.88 with the `Project` kept, by building `Analyser::declarations()`
over `public_symbols` + `find_all_references` and running it on the corpora.

**`public_symbols` is the public surface only.** For an entity whose architecture declares
`signal orphan` and `constant dead`, the query returns six declarations — the entity, its two
ports, the architecture, and two `std.standard` types — and neither of the dead ones. An
architecture's internal declarations are not public symbols, so a cross-file reference graph
built this way cannot see the objects whose deadness would be worth reporting.

**The surface it does reach is noise.** "Declared and never referenced" over it gives 521
findings on open-logic: 203 architectures (never named by anything, by construction), 252
constants and 35 array types — a library's public API, which exists to be used from outside it.
Narrowing to architecture-local objects gives zero on all three corpora, but only because the
query cannot see them at all; a positive-control file confirmed that rather than the code
being clean.

**Consequence.** Both remaining PR 8+ items need a custom `ast::search::Searcher` over
`vhdl_lang`'s AST rather than the `Project` query API, because `DesignRoot` is not public
(§10.6). That is the real size of the work, and it is more than "small-to-medium".

Recursion in particular cannot be done syntactically: scanning for a function whose body names
itself finds 1871 candidates in VUnit and 3397 in cnn_accel, nearly all of them overload
resolution (`to_slv` calling a different `to_slv`) rather than recursion. Telling those apart is
exactly what the resolved call graph is for.

### Extension repository `vsg-rs-vscode`

**VS 1 — skeleton.** Manifest, VHDL activation, `LanguageClient`, `systemPath`/`userPath` only,
development instructions. Proves protocol integration with no bundling. **Small.**
**VS 2 — embedded binaries.** Platform-specific VSIX per target, download-by-tag with `SHA256SUMS`
verification at package time, graceful unsupported-platform message, version reporting. **Medium.**
**VS 3 — formatter UX and docs.** Default-formatter configuration, format-on-save, coexistence with
`vhdl_ls`. **Small.**
**VS 4 — release automation.** CI packaging, smoke tests, Marketplace + Open VSX publication.
**Medium.**

---

## 7. Parallelization

```text
PR 1 ─┬─> PR 2                     (independent once PR 1 lands)
      └─> PR 5
PR 3 ──> PR 4 ──> PR 5 ──> PR 6 ──> VS 1 ──> VS 2 ──> VS 3/VS 4
PR 7 and PR 8+ are independent of the LSP line and can proceed in parallel.
```

PR 1 and PR 3 touch overlapping files and should **not** run concurrently. PR 2 and PR 3 can.
The extension repository is fully independent once PR 5 exists.

## 8. Dependency graph

```text
One diagnostic model (PR 1)
   ├──> SARIF related locations + fixes (PR 2)
   ├──> LSP diagnostics (PR 5)
   └──> shadowing / multiple drivers with related spans (PR 8+)

Analysis in the library (PR 3) ──> in-memory analysis (PR 4) ──> LSP (PR 5) ──> code actions (PR 6)

Formatter library API (exists) ─────────────────────────────> LSP formatting (PR 5)

Release binaries (exist) ───────────────────────────────────> embedded VS Code packaging (VS 2)
```

Two of the brief's candidate PRs are **already satisfied** and are not planned: a structured fix
representation (exists, `src/rules/mod.rs:107`) and reusable in-memory formatter entry points
(exist, `src/lib.rs:290`).

## 9. First implementation batch

**PR 1, PR 3, PR 4, PR 5** — in that order, with PR 2 opportunistically after PR 1.

Rationale: PR 1 is the only change every downstream consumer needs and is the one that gets harder
the longer it waits, because each new rule adds another construction site. PR 3 and PR 4 are the
two reasons an LSP cannot be built today, and neither changes behaviour, so they are cheap to
review and safe to land. PR 5 then becomes a thin adapter rather than a feature. The VS Code
extension starts only once `vsg-rs lsp` speaks the protocol, so that VS 1 can be validated against
a real server.

PR 4's unknown has been resolved by the spike (§10.6): the overlay API exists and is the one
vhdl_ls itself uses, so no fallback is needed.

---

## 10. Analysis-fact duplication (audited)

### 10.1 Redundant work per `--check lint` run

* **The style layer runs even when only lint was asked for.** Results are computed at
  `src/vsg_cli.rs:1689-1740` and then discarded: `if !style { … r.violations.clear() }`
  (`:1758-1763`).
* **Four `vhdl_syntax` parses per file** (five when formatting changes anything), plus one
  independent `vhdl_lang` parse: the cross-file style index (`:1129`), the style check (`:426`),
  the format re-parse (`rules/mod.rs:209`), `elaborate::entities` (`elaborate.rs:78`) and
  `lint_files` (`:1341`). The last two parse **the same bytes thirty lines apart in the same
  function**.
* **Nine full-tree walks** in the lint parse, five of them the identical
  `find(root, ArchitectureBody)`.
* **`assignments()` does eight full subtree walks per call** (`design.rs:98-102`), from eleven call
  sites, several inside loops.
* **One quadratic path**: `fsm.rs:108` takes `all_tokens` of the whole architecture once per
  assignment.

### 10.2 Duplication

* `src/testbench.rs:60-73` is a **verbatim copy** of `design.rs:20-33` (`find`), and `code_of`
  (`:140-155`) re-implements `text_of`. It imports nothing from `design`.
* **Six copies** of "lowercase token text": `clockdomain.rs:27`, `width.rs:51`, `width.rs:116`,
  `fsm.rs:33`, `design.rs:294`, `elaborate.rs:169`.
* **Three clocked-process detectors**: `design.rs:178` `is_clocked`, `design.rs:186`
  `is_not_combinational` (which repeats the same substring tests at `:197-198`), and
  `clockdomain.rs:36` `clock_of`.
* **Four assignment-kind lists**: `design.rs:81`, `combinational.rs:131`, `clockdomain.rs:97`,
  `width.rs:109` — all different subsets.
* **Five declaration readers**, none shared: `elaborate.rs:161`, `elaborate.rs:42`, `width.rs:45`,
  `fsm.rs:38`, `design.rs:286`. Each re-does "the tokens before the colon are the names".
* **Two range parsers** with different whitespace assumptions: `design.rs:448` `static_range`
  (`"7 downto 4"`) and `width.rs:24` `width_of` (`"7downto0"`).
* Entity-name-from-instantiation exists twice: `elaborate.rs:107` and `clockdomain.rs:74`.

### 10.3 The style layer already solved this

`rules/mod.rs:149-158` builds `by_kind: HashMap<NodeKind, Vec<SyntaxNode>>` in **one** stack walk,
exposed as `Context::nodes(kind)`, with `OnceCell` fields for derived facts. `design::find` is
exactly the un-indexed version of it. PR 7 should give the lint layer the same `Context`, not
invent a new abstraction.

### 10.4 What stays bespoke

Three rules genuinely need their own traversal and should not be forced into fact queries:
`lint_600` latch inference (`design.rs:206` `always_assigned` is path-sensitive over
if/elsif/else/case), `variable_latches` (`design.rs:339`, needs read/write *ordering*), and
`lint_711` (`fsm.rs:168`, needs per-alternative assigned *values*).

### 10.5 Existing project-level facts

Two, both deliberately partial: `elaborate::Entities` (`elaborate.rs:35`) — entity/component name
to port modes and order, **no types or widths**, explicitly "not elaboration in the LRM sense"
(`elaborate.rs:10-12`); and `vhdl_lang`'s analysed `Project` (`lint.rs:470`), whose **syntax tree
and symbol information are dropped** — `Analysis` keeps only findings (`lint.rs:35-41`). No
project-level structure holds signals, drivers or widths across files.

**This is the single biggest missed opportunity in the codebase**: a fully resolved symbol table is
built, used for 56 diagnostics, and thrown away. Most of the brief's proposed analyses want exactly
that table.

### 10.6 Spike result: what `vhdl_lang` 0.88 actually exposes

Investigated against the vendored source. **The information is reachable; vsg-rs discards the
`Project` value, not the data.** `Project` keeps the analysed root alive and exposes a semantic
query API on it.

Reachable and cheap:

* `Project::find_all_entity_references(&Source) -> Vec<(SrcPos, EntRef)>` (`project.rs:344`) —
  every resolved position in a file with the entity it resolves to, **in one call**.
* `find_declaration`, `find_definition`, `item_at_cursor`, `find_all_references(ent)`,
  `public_symbols`, `document_symbols`, `find_implementation` (`project.rs:275-353`).
* `AnyEnt` (`named_entity.rs:264`) has public `id`, `parent`, `designator`, `decl_pos`,
  `src_span`, `kind`, plus `path_name()`, `signature()`, `is_subprogram()`.
* A public visitor: `ast::search::{Searcher, Search}` (`ast/search.rs:85,121`), implemented across
  the whole AST.

**In-memory analysis is a supported path, not a workaround.** `Source::inline(path, text)`
(`data/source.rs:140`) plus `Project::update_source` (`project.rs:201`) plus `analyse()` is exactly
what vhdl_ls uses for unsaved buffers; `update_config` documents re-parsing "from in-memory source
(required for incremental document updates)". **PR 4's main unknown is resolved: no temporary files
are needed, and its risk drops from medium to low.**

Limits found:

* `DesignRoot` is **not public**, so every prebuilt searcher (`ItemAtCursor`, `FindAllEnt`,
  `FindAllReferences`) is public-in-name-only — their constructors take `&DesignRoot`. A custom
  `Searcher` is required, and an `EntityId -> EntRef` map must be built by hand from
  `find_all_entity_references` plus `public_symbols`.
* `SourceFile` exposes only `num_lines()`; keep your own path list and use `get_source(&Path)`.
* `ErrorCode` is not exported, confirming the existing `format!("{:?}")` workaround in `lint.rs`.

Consequences for the later PRs:

| Proposed analysis | Verdict | Why |
|---|---|---|
| Declarations with zero references | **cheap** | `find_all_entity_references` gives it directly; `enable_unused_declaration_detection` already surfaces it as `lint_004` |
| Subprogram call graph | **medium** | callee side is exact and overload-resolved; the caller side needs our own scope-containment tracking, as `Searcher` has no enter/leave events |
| Unused `use` clauses | **large, not cheap** | clauses and their resolved targets are visitable, but **nothing records whether a visibility was consumed** — it must be reimplemented by intersecting each package's region with the unit's referenced ids |
| Exact case analysis | **blocked as specified** | `Choice` (`ast.rs:217`) carries **no resolved type**; types are not written back for choices. The selector's type is only reachable indirectly through an object's subtype. This is not the cheap semantic win the brief assumes |
| Shadowing | **medium** | `AnyEnt::parent` and `path_name()` give the scope chain; region contents come from matching exported `AnyEntKind` variants |

So the ordering changes: **unused imports and case analysis are no longer early, cheap wins**, and
zero-reference and call-graph work moves ahead of them.

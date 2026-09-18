# vsg-rs next to Linty and Sigasi

vsg-rs is a formatter and a style linter: it parses each file on its own with `vhdl_syntax` and
reports what the text says. Linty and Sigasi are design-analysis tools that build a model of the
design first. This page lists what that difference costs, so the gap is a decision rather than a
surprise. Everything about the two products below comes from their public documentation
(read 2026-09-18); items their documentation does not answer are marked as such.

## The three tools in one paragraph each

**vsg-rs** — open source (MIT/Apache-2.0), reimplements VSG 3.35's ~972 rules, formats and fixes
nearly all layout and style violations, VSG-compatible YAML, SARIF, JUnit and GitLab
code-quality output, a GitHub Action, one binary, no project model.

**Linty** — a commercial SonarQube distribution with VHDL (180 rules), Verilog/SystemVerilog
(235) and language-agnostic HDL (62) plugins. Three engines: a source linter, *BugFinder*, which
**synthesizes the design** and checks the netlist, and a *Formal Verification* engine running
SymbiYosys. Priced by lines of code (€8k–€48k/year, plus roughly the same again for the Ultra
tier holding BugFinder and formal); free for open source, and a reduced free VS Code tier.
Published DO-254 and CNES rule mappings, a Qualification Kit, pull request decoration in
GitHub/GitLab/Bitbucket. No auto-fix is documented anywhere.

**Sigasi Visual HDL** — a commercial VS Code extension on a full incremental VHDL/SystemVerilog
front end with libraries, design hierarchy and binding. 262 numbered VHDL rules, published with
ids. Quick fixes applicable to a file, a library or the whole project, a formatter, rename
refactoring, and a headless CLI (`verify`, `format`, `compilation-order`) with JSON,
SonarQube-generic and Warnings-NG output. The community edition is free for non-commercial use;
the headless CLI needs the Enterprise (CI) licence.

## The gap, by what it needs

### A synthesized netlist — Linty BugFinder only; Sigasi has no equivalent either

Latch inference, combinational loops, clock-domain and reset-domain crossings, clock and reset
domain properties (mixed edges, mixed reset polarity, flip-flops without a reset), FSM extraction
with unreachable, deadlock and livelock states, constant or directly tied outputs, registered
boundary checks, and formal properties (overflow, memory range, user-supplied SymbiYosys checks).

Out of scope for vsg-rs: that is a synthesis tool wearing a linter's hat.

### Multi-file elaboration and library binding — both products

Unbound component instantiation, component-versus-entity mismatch, configuration binding,
duplicate design units, port and generic maps checked against the real entity, project-wide name
uniqueness, unused entities, architectures and generate blocks, clock and reset names preserved
across the design, compilation order and hierarchy export, rules that know the top level.

vsg-rs today: none of it. Its consistency rules see only the files given in one run, and match by
name rather than by binding.

### Type checking and static evaluation — both products

Type and subtype checking, vector width mismatches, static index out of range, case completeness
against the type's value set, overlapping or out-of-range choices, assignment to an input port or
a constant, function purity, missing bodies, incomplete record aggregates.

vsg-rs today: none of it.

### Dataflow analysis — both products

Incomplete, superfluous or duplicate sensitivity lists; signals never read or never written; read
before assignment; multiple drivers; dead code and dead states; unused ports, generics and
declarations.

vsg-rs today: none of it. Of this group, the single-file cases — sensitivity lists, never-read
signals, unused declarations inside one architecture — are the realistic next step, because they
need name resolution within a file rather than a model of the design.

### Platform capabilities

A server-side issue lifecycle with quality gates, new-code-only gating and persistent
accept/false-positive states (Linty, inherited from SonarQube); published per-rule standards
mappings and a purchasable Qualification Kit (both); simulation-versus-synthesis rule scoping
from declared testbench paths (Linty) and an RTL-versus-all-code severity split (Sigasi); fixes
applied at library or project scope and rename refactoring (Sigasi); simulator coverage
decoration (Linty); block, FSM and UVM diagrams.

vsg-rs has file-scoped configuration, inline `vsg_off` waivers and CI exit codes, and nothing
that remembers a triage decision between runs.

## Where vsg-rs is ahead

* **Fixing.** vsg-rs fixes nearly every style and layout rule it reports. Linty documents no
  auto-fix at all; Sigasi's quick fixes and formatter are comparable, but only inside its editor
  or its licensed CLI.
* **Output formats.** Neither product offers SARIF or GitLab code-quality JSON. Sigasi has JSON,
  SonarQube-generic and Warnings-NG; Linty has its own server plus a static HTML report.
* **Getting started.** One binary or `pip install vsg-rs`, a GitHub Action, no server, no project
  file, no licence. Linty needs Docker, PostgreSQL and a licence key; Sigasi's headless mode
  needs the Enterprise (CI) licence.
* **VSG compatibility.** Rule ids, configuration and reports match VSG 3.35, which neither
  product attempts.
* **Cost.** Zero, against €8k–€48k a year (Linty) or a per-seat licence (Sigasi, prices not
  published).

## Language coverage

Sigasi documents itemised VHDL-2019 support, mode views included. vsg-rs parses VHDL-2019
conditional analysis, generic subprograms and interface packages, but not mode views, not
conditional expressions in declarations, and not PSL written as code; 3 of 3000 corpus files
(0.1%) fail to parse for such reasons and are then reported and left untouched
(`compatibility.md`). Linty's documentation does not state which VHDL revisions it accepts.

## What this means for vsg-rs

In rough order of value per effort:

1. **Single-file semantic rules** on top of name resolution: incomplete sensitivity lists,
   signals never read or never written, unused declarations, case completeness for locally
   declared enumerations. This is the one group where vsg-rs can close a real gap without
   becoming a different tool.
2. **A design model** (libraries, binding, cross-file types) would unlock the elaboration and
   type-checking groups. It is a large project, and `vhdl_ls` already maintains one, so the
   sensible move is to use it rather than rebuild it.
3. **Netlist and formal analysis** belong to synthesis tools; vsg-rs should not grow one.
4. **Triage state** — findings accepted once and staying accepted between runs — is the cheapest
   platform feature to copy and the one CI users ask for most.

Positioning, stated plainly: Linty and Sigasi answer "is this design correct?". vsg-rs answers
"does this code look the way we agreed?", and on that question it repairs instead of reporting.
A project can run both.

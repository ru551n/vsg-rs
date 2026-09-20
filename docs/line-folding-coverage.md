# Line-folding coverage

Status of automatic folding per syntax class. Statuses: **supported** (folds at structural
boundaries, golden-tested), **partial** (folds, but some sub-forms do not or the layout is not
final), **unavoidable overflow only** (no legal break point exists), **blocked by frontend**,
**planned**.

Test references are fixtures in `tests/formatting/`.

| Syntax class | Status | Notes | Tests |
|---|---|---|---|
| Design unit headers (`entity x is`, `architecture a of e is`, `package … is`) | unavoidable overflow only | only identifiers; no legal break worth taking | — |
| Library and use clauses | partial | multiple names fold after `,`; a single selected name is an unavoidable overflow | `unbreakable`, `configuration_context` |
| Context declarations and references | supported | | `configuration_context` |
| Entity generic list | supported | always one per line, aligned | `entity_interface` |
| Entity port list | supported | always one per line, aligned, modes padded; long subtypes and defaults fold inside | `entity_interface` |
| Multiple names in one interface declaration | partial | identifier list folds after `,`; very wide lists are excluded from alignment | corpus |
| Subprogram parameters | supported | flat or one per line, aligned when broken | `declarations` |
| Function return / `is` after parameters | supported | stays after `)` | `declarations` |
| Function call | supported | one argument per line; nested calls fold independently | `calls_aggregates`, `deep_nesting` |
| Procedure call | supported | | `calls_aggregates` |
| Generic map | supported | always one per line, `=>` aligned | `instantiation` |
| Port map | supported | always one per line, `=>` aligned; long actuals fold after `=>` or inside | `instantiation` |
| Positional maps, `open`, comments in maps | supported | | `instantiation` |
| Entity / component / configuration instantiation | supported | long library/entity names are unavoidable overflow | `instantiation` |
| Aggregate (named, positional, nested, `others`) | supported | named: one per line, aligned; literal-only: filled | `calls_aggregates` |
| Qualified expressions, type conversions | supported | folded like aggregates / calls | `calls_aggregates` |
| Array type declaration | supported | breaks before `of`, then inside index lists | `declarations` |
| Record type declaration | supported | one element per line, `:` aligned | `declarations` |
| Enumeration type | supported | one literal per line when broken | `declarations` |
| Physical type | supported | units always one per line | `declarations` |
| Signal / variable / constant declaration | supported | breaks after `:` for long subtypes and before/after `:=` | `declarations`, `calls_aggregates` |
| File declaration | supported | breaks before `open` / `is` | `declarations` |
| Alias declaration | supported | breaks before `is` | `declarations` |
| Attribute declaration / specification | supported | breaks before `is` | `declarations` |
| Subtype declaration / range constraints | supported | range folds before `to` / `downto` | `declarations` |
| Component declaration | supported | | `instantiation` |
| Group / group template declaration | partial | template class list folds as a list; group declaration names do not fold | — |
| Disconnection specification | partial | breaks before `after` | — |
| Simple signal / variable assignment | supported | expression folds; value moves down if its first line cannot fit | `assignments`, `unbreakable` |
| Waveforms with `after` | supported | elements one per line, aligned | `assignments` |
| Conditional signal / variable assignment | supported | one branch per line; conditions fold | `assignments` |
| Selected signal / variable assignment | supported | one alternative per line; choices fold | `assignments` |
| Force / release assignments | partial | generic assignment layout | — |
| Boolean expressions | supported | before each operator, operator first | `assignments`, `conditions` |
| Relational / shift / arithmetic / concatenation | supported | precedence-aware | `assignments` |
| Unary expressions, `abs`, `not`, `??` | supported | never split from the operand | `assignments` |
| Parenthesized expressions | supported | aligned inside | `assignments`, `deep_nesting` |
| If / elsif conditions | supported | aligned after the keyword, `then` stays on the last line | `conditions` |
| While condition | supported | | `conditions` |
| For loop / for generate range | supported | range folds; bounded alignment | `generate_case_loop` |
| If / elsif / else generate (with alternative labels) | supported | | `generate_case_loop` |
| Case generate | supported | | `generate_case_loop` |
| Assert / report / severity | supported | clauses always on their own lines; message folds at `&` | `assert_report_wait` |
| Report statement | supported | breaks before `severity` | `assert_report_wait` |
| Case statement choices | supported | before each `\|` | `generate_case_loop` |
| Wait statement | supported | clauses fold onto indented lines | `assert_report_wait` |
| Return / next / exit | supported | expressions and `when` conditions fold | `conditions` |
| Process sensitivity list | supported | one name per line when broken | `generate_case_loop` |
| Block statements (guard, header) | supported | | `generate_case_loop` |
| Configuration declarations / specifications | supported | binding indications and maps indented | `configuration_context` |
| Protected types, package instantiation | supported | | `protected_types` |
| Selected / indexed names, external names | unavoidable overflow only | names are never split; indices fold as lists | — |
| Signatures (`[t return t]`) | supported | move to a continuation line, then one type mark per line | `signatures` |
| VHDL-2019 tool directives | supported | kept on their own line at column 0 | `tool_directives` |
| PSL | blocked by frontend | PSL as code does not parse; the file is left untouched and named as PSL. PSL in comments folds like any comment. The lint layer reads both | — |
| Comments (trailing, own-line, block) | supported | never moved or wrapped; trailing comments force a break | `comments` |
| Formatter-off regions | supported | items inside are kept as written | `fmt_off` |
| Nested constructs | supported | bounded alignment prevents staircases | `deep_nesting` |
| Malformed / recovered syntax | not formatted | file is left untouched, error reported | `tests/cli.rs` |

## Measured on a real-world corpus

`cargo run --release --example corpus -- DIRS` over every VHDL file below a development
directory (11,749 files, 134.6 MB: tsfpga and hdl-modules, VUnit and OSVVM, including copies in
virtual environments, nvc's libraries and regression tests, rust_hdl, project code),
September 2026:

| Width | Files formatted | Internal errors | Unstable | Code lines still too long |
|---|---|---|---|---|
| 120 | 11,603 | 0 | 0 | 649 |
| 80 | 11,603 | 0 | 0 | 11,938 |
| 40 | 11,603 | 0 | 0 | 273,249 |

The other 146 files have syntax errors (mostly deliberate, in parser regression tests) or PSL
and are left untouched. `FIX=1` (safe fixes) and `FIX=unsafe` report 0 internal errors and 0
unstable files as well: a second `fix` run changes nothing.

At width 120, the remaining long lines inspected were single string literals, long selected
names, or declarations whose identifier and type mark alone exceed the width. Those are
unavoidable overflows. At narrower widths the count is dominated by long identifiers and
indentation depth.

**Not yet claimed**: review of the real-world long-line classification at widths below 80.

The randomized tests in `src/fuzz.rs` check that layout does not depend on source whitespace and
that token deletions never cause an internal error. Set `VSG_FUZZ_DIRS` to run them over a
local corpus.

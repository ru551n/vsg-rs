# Roadmap

Direction, not a wish list. Everything here is something the project intends to do; anything
already shipped is documented on its own page instead, and anything nobody is working on is left
out rather than recorded as an aspiration.

For what exists today, see [static analysis](lint.md) and the
[rule reference](rule-reference.md).

## Static-analysis facts

The analyser currently derives names, types, control flow within a process, and the port
connections between design units. The work with the most leverage is extending that set of facts,
because each new fact makes several rules possible at once:

* **A project reference graph** — every place a declaration is read or written, across files.
  Several rules approximate this per file today and are narrower than they need to be.
* **A subprogram call graph**, which makes recursion reportable. Subprograms nothing calls are
  already reported, by the front end's `lint_004`.

## Interface consistency

Ports and generics are checked against the entity today. The same treatment for component
declarations against their entities, and for configurations, is a natural extension of the
elaboration data already collected.

## Frontend completeness

vsg-rs is limited by what its parser accepts. VHDL-2019 support and the
[remaining gaps](vhdl-frontend.md) are upstream work in `vhdl_syntax`; the roadmap item here is to
track it and remove the workarounds as they become unnecessary.

## Not planned

Stating these saves everyone time:

* elaboration, synthesis, timing or resource estimation — see [non-goals](index.md#what-it-is-not);
* a custom rule language. Local rules work through VSG's own plugin mechanism;
* certification or compliance mappings;
* editor plugins beyond the [documented ones](editors.md) — every editor with a language client
  can run `vsg-rs lsp`, and every editor that pipes a buffer through a command can run
  `--stdin --fix`. Both contracts are generic, so a bespoke plugin per editor earns nothing;
* heuristic rules that cannot reach zero findings on the validation corpora.

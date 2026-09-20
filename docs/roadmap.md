# Roadmap

Direction, not a wish list. Everything here is something the project intends to do; anything
already shipped is documented on its own page instead, and anything nobody is working on is left
out rather than recorded as an aspiration.

For what exists today, see [static analysis](lint.md) and the
[rule reference](rule-reference.md).

## Static-analysis facts

The analyser derives names, types, control flow within a process, and the port connections
between design units. The front end contributes a fully resolved symbol table on top of that,
which is where most of the facts a linter wants already live.

One is missing:

* **A subprogram call graph**, which would make recursion reportable. Recursion is legal VHDL
  and unsynthesisable, so it is worth saying. It cannot be done from syntax: a function whose
  body names itself is nearly always overload resolution rather than recursion — 1871 such
  candidates in VUnit, almost none of them recursive — so it needs the resolved call graph and
  nothing less.

A project-wide reference graph was the other candidate and turned out not to be needed: the
front end's `lint_004` already reports a declaration nothing uses, including signals and
constants local to an architecture. It needs the library map, like every resolved-semantic rule;
see [project setup](project-setup.md).

## Interface consistency

Ports and generics are checked against the entity, and `lint_750` checks component declarations
against theirs. Configurations are not checked: a configuration naming an instance label the
architecture does not have, or an architecture the entity does not have, is not reported.
Nothing is written here about when that will change — configurations appear in 11 files of the
1,864 across the validation corpora, and the work needs facts the elaboration pass does not
collect yet (architecture names per entity, instance labels per architecture).

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

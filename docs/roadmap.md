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
* **Case analysis against the type's value set**, so a missing choice is reported from the type
  rather than from the shape of the statement.
* **A subprogram call graph**, which makes recursion and unreachable subprograms reportable.
* **Shadowing and visibility**, reported from the symbol table rather than from syntax.

## Related locations

A finding currently names one position. Multiple drivers, shadowed declarations and combinational
loops are all statements about several places at once, and the report formats
([SARIF](reports.md) in particular) can carry them. Findings should say "here, and also here".

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
* additional editor integrations beyond the [documented ones](editors.md) — the stdin contract is
  stable and generic, and editors are better served by it than by bespoke plugins;
* heuristic rules that cannot reach zero findings on the validation corpora.

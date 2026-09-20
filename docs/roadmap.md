# Roadmap

Direction, not a wish list. Everything here is something the project intends to do; anything
already shipped is documented on its own page instead, and anything nobody is working on is left
out rather than recorded as an aspiration.

For what exists today, see [static analysis](lint.md) and the
[rule reference](rule-reference.md).

## Interface consistency

What a design says about itself in more than one place, checked against itself. Ports and
generics are compared with the entity, `lint_750` compares component declarations with theirs,
and `lint_751` checks the architecture names a configuration uses.

One part of a configuration is still unchecked: the instance label, and the component named
beside it. `for i_dff : dff` is not compared against the architecture it configures, so a label
that no longer exists, or one whose instance is of a different component, is not reported. It
needs a fact the elaboration pass does not collect — the instance labels of every architecture —
and configurations are rare enough (11 files of the 1,864 across the validation corpora) that
this is recorded rather than scheduled.

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

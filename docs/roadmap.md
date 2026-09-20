# Roadmap

Direction, not a wish list. Everything here is something the project intends to do; anything
already shipped is documented on its own page instead, and anything nobody is working on is left
out rather than recorded as an aspiration.

For what exists today, see [static analysis](lint.md) and the
[rule reference](rule-reference.md).

## Design reports

vsg-rs reports what is wrong. It does not yet report what a design *is*, and the two are
different products. A rule says "this cannot work". A report says "here is what you have
built", and leaves the judgement to the reader.

That distinction is why the report is worth building. A default run reports definite errors
only, so the analysis behind clock domain crossings, latches, state machines and combinational
cycles is switched off unless someone asks for it: the conclusions depend on intent, even
though the facts do not. A report is where those facts belong. They are already computed and
have nowhere to go.

Most of the work is keeping what is already worked out rather than working out anything new.
`clockdomain`, `fsm` and `combinational` each expose one function returning a list of findings,
and discard everything they learned on the way: which clock a register belongs to, a machine's
states and the transitions between them, the signals on a cycle. `elaborate` is the exception
and already publishes its facts as data, which is why hierarchy is nearly free.

| Report | Facts today | What it needs |
|---|---|---|
| Hierarchy | published as `elaborate::Design` | presenting |
| State machines | computed, discarded | returning them |
| Latches | computed, discarded | returning them |
| Combinational cycles | computed, discarded | returning them |
| Clock domain crossings | computed, discarded | returning them, with each register's clock |
| Reset domain crossings | nothing | inferring resets, which nothing does today |

Reset domain crossings are the odd one out, and the only item here that is new analysis rather
than a new way of presenting old analysis. Nothing in vsg-rs infers which signal is a reset;
[static analysis](lint.md) lists that among the things it does not try to work out.

The shape is undecided. The first step that settles it is hierarchy and state machines behind a
`--report`, hierarchy because it needs no analysis change and proves the output, state machines
because they prove the pattern the other three then follow. Machine readable first: a table for
a person can be produced from structured output, and not the other way round.

## Interface consistency

What a design says about itself in more than one place, checked against itself. Ports and
generics are compared with the entity, `lint_750` compares component declarations with theirs,
and `lint_751` checks both the architecture a configuration names and the instances inside it.

Configurations are the last piece of this, and they are now checked as far as the elaboration
pass can see. What remains is the nested case: a configuration written inside another one
configures a block or a generate, whose labels are its own, and those are left alone rather
than read as the architecture's.

## Drivers the language does not permit

`lint_601` reports a signal driven by more than one concurrent statement. That is legal for a
*resolved* type, since deciding between the drivers is exactly what a resolution function is
for, so the rule is advisory. Two drivers on an **unresolved** type are a different matter: nothing
decides between them, and the design cannot elaborate. That would be a definite error.

**It cannot be built today, and not for want of trying.** Asking whether a subtype is resolved
means asking the front end, and `vhdl_lang` 0.88 does not model the answer:

* `ResolutionIndication` exists in its AST (`src/ast.rs`), but appears nowhere under
  `src/analysis/`: the resolution function is parsed and then dropped;
* `Subtype` (`src/named_entity/types.rs`) carries a type mark and nothing else, and
  `Type::Subtype(Subtype)` adds nothing;
* so there is no way to ask whether `std_logic` is resolved and `std_ulogic` is not.

The two ways round it are both worse than not having the rule. Recognising the resolved types by
name is guessing from spelling, which is what this layer exists not to do, and re-deriving
resolution from the IEEE sources means a second implementation of VHDL type semantics living
here. The rule waits for the front end to keep what it already parses.

## Frontend completeness

vsg-rs is limited by what its parser accepts. VHDL-2019 support and the
[remaining gaps](vhdl-frontend.md) are upstream work in `vhdl_syntax`; the roadmap item here is to
track it and remove the workarounds as they become unnecessary.

## Not planned

Stating these saves everyone time:

* elaboration, synthesis, timing or resource estimation. See [non-goals](index.md#what-it-is-not);
* a custom rule language. Local rules work through VSG's own plugin mechanism;
* certification or compliance mappings;
* editor plugins beyond the [documented ones](editors.md). Every editor with a language client
  can run `vsg-rs lsp`, and every editor that pipes a buffer through a command can run
  `--stdin --fix`. Both contracts are generic, so a bespoke plugin per editor earns nothing;
* heuristic rules that cannot reach zero findings on the validation corpora.

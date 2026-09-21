# Editing VHDL in VS Code

The VS Code extension does two separate jobs. It runs the [vsg-rs language server](lsp.md) for
diagnostics, quick fixes and formatting. And it adds **editing actions**: instantiating an entity,
declaring the signals a port map needs, generating a state machine, adding a `use` clause, and the
like. This page is about the second job, which works differently and has a requirement the first
does not.

## What it needs: VHDL-LS

The editing actions need **[VHDL-LS](https://github.com/VHDL-LS/rust_hdl) (`rust_hdl`), running,
with a `vhdl_ls.toml` for the workspace.** Every entity, port, type and library name they write
comes from it. The vsg-rs server cannot stand in: it answers no request about what a name means,
by design (see [architecture](architecture.md#the-boundary-with-vhdl_ls)).

Without it:

* every editing command reports that no language server answered, and does nothing;
* names stay uncoloured, because the grammar deliberately leaves every identifier to VHDL-LS's
  semantic tokens;
* **lint and format are unaffected.** They come from a different server and keep working.

The extension does not declare VHDL-LS as a dependency, so installing it does not pull one in. The
[VHDL-LS extension](https://marketplace.visualstudio.com/items?itemName=hbohlin.vhdl-ls) is the
usual way to have one running.

A minimal `vhdl_ls.toml`, at the workspace root:

```toml
[libraries]
mylib.files = ['src/**/*.vhd']
```

!!! warning "Not `work`"
    `work` is reserved in `vhdl_ls.toml`. A library named `work` there is silently ignored, the
    server falls back to analysing only the files that are open, and every command here then finds
    nothing. Give the library any other name.

## What it adds

**Instantiate Entity** picks any entity in the workspace and writes the instantiation:

```vhdl
i_leaf : entity work.leaf
  generic map (
    g_width => g_width
  )
  port map (
    clk  => clk,
    din  => din,
    dout => dout
  );
```

The library is spelled the way the file needs it. Inside the library the file is itself analysed
in, that is `work`. From any other library it is that library's name, and the `library` clause
that makes the name visible is added with it, after the existing context clause. The library is
the one the language server analysed the entity in, not a guess.

The same thing is offered as a completion while you type an entity name, as a snippet you can tab
through.

**Declare Signals for Port Map** takes the instantiation under the cursor and declares every
actual that does not exist yet, with the port's type, before the enclosing `begin`. Generics used
in a type are substituted with the values from the generic map, falling back to their defaults:

```vhdl
signal din  : std_logic_vector(16 - 1 downto 0);
signal dout : std_logic_vector(16 - 1 downto 0);
```

Ports mapped to `open`, to an expression or to an already declared name are skipped.

**Create State Machine from Enum Type** puts the cursor on an enumeration type and generates the
state signal and a registered process with a `case` arm per literal. The signal is declared after
the type, which may span several lines, and the process is added after the architecture's own
`begin`, because a process cannot sit among declarations. The clock and reset are taken from the
entity's own ports; the reset style is your choice of synchronous, asynchronous or none. The type
has to be declared in an architecture.

**Add Library and Use Clause for Symbol** is the VHDL answer to "add import". Put the cursor on a
name, or take the lightbulb on an unresolved-name error, and every package the server knows that
declares it is offered as a list:

```
ieee.numeric_std        function to_unsigned[...]
ieee.std_logic_arith    function to_unsigned[...]
```

Picking one inserts the `use` clause after the existing context clause, adding `library ieee;`
only if it is not already there, and only for the design unit the cursor is in. Packages that are
already visible are left out of the list, and candidates are ordered `ieee` first, then
alphabetically, with `work` last.

**Completion** offers two things the server does not: an entity, which inserts the whole
instantiation as a snippet, and a name from a package this design unit has not made visible yet,
which inserts the `use` clause along with it.

**Inlay hints** show each port's direction and type next to the actual in a port or generic map,
so a long map reads without jumping to the entity.

**Map missing ports** is a quick fix on an instantiation whose port map is incomplete: it appends
the formals that are not associated yet.

**References CodeLens** sits on every entity declaration and opens the list of places that refer
to it.

**Signature help** shows the port list while a map is being typed, with the formal under the
cursor highlighted.

**Remove Unused Use Clauses** reports the context clauses that nothing in the design unit resolved
to, and removes the ones you select. It never removes anything on its own.

**Declare Entity as Component** writes the matching `component` declaration for old style
instantiation.

**Extract to constant / signal** turns a selected expression into a declaration before `begin` and
replaces the selection with its name. It edits the document the selection was made in.

**VHDL Design** is a tree in the Explorer of every entity in the workspace, expanding through its
architectures into the instances they contain, each resolved to the entity it instantiates. It
appears once the workspace holds VHDL.

Every command is in the command palette under **VHDL:** and is named `vsg-rs.<command>`, for
example `vsg-rs.instantiateEntity`.

## Syntax colouring

The extension contributes the `vhdl` language definition, a TextMate grammar, and colour themes:
**Gruvbox VHDL Dark** and **Gruvbox VHDL Light**, and forty-one dark ones named for the schemes
they take their colours from, such as **Tokyo Night VHDL Night**, **Catppuccin VHDL Mocha** and
**Kanagawa VHDL Wave**. Choose one with *Preferences: Color Theme*. The grammar covers only what VHDL-LS never
classifies: keywords, comments, strings, literals, attributes and operators. Every name is
coloured from its semantic token, so constants and generics, enumeration literals, record types
and instantiation labels are distinguished by what the analyser resolved them to, not by how they
are spelled. Before a project has a `vhdl_ls.toml`, names stay uncoloured.

## How it works

**No VHDL is parsed.** Entities, types, ports and libraries are read back from the language server
through VS Code's standard provider commands (`executeWorkspaceSymbolProvider`,
`executeHoverProvider`, `executeDocumentSymbolProvider`, `executeDefinitionProvider`,
`executeImplementationProvider` and `executeReferenceProvider`), and the generators transform the
server's own output. That is a constraint, not an accident: a second VHDL parser in an extension
would agree with the server about nothing.

The generators are plain functions with no VS Code dependency, checked against hover text taken
verbatim from a real server. `scripts/probe.mjs` in the extension dumps what VHDL-LS answers for a
given project, which is how those fixtures were produced.

Two facts about the server shaped the code, and are worth knowing if you change it:

* **A workspace symbol query returns at most 200 symbols**, applied to the first matches in
  library order, before ranking. Every port, generic and architecture counts toward that, so a
  project of a few dozen entities cannot be listed with an empty query. The entity picker and the
  hierarchy therefore ask each file for its own symbols instead, which are not capped. Completion
  and the use-clause search do query by name, so a very short or very common name may not list
  every candidate.
* **A resolution that fails is not remembered.** "This is not an instantiation of anything the
  server knows" and "the server has not finished analysing" look the same from outside. Caching
  the second would leave a file opened during start-up without hints until its next edit.

## Known limits

* Signal types are copied from the port with generic names substituted textually. A port whose
  type is only fixed by elaboration is emitted as written.
* The port map is read for `formal => actual` pairs using the formal names the server reported.
  Positional association is not handled.
* The context clause is recognised by matching `library` and `use` at the start of a line, so a
  clause split across lines can produce a duplicate.
* The design hierarchy lists every entity at the top level, not only the ones nothing
  instantiates. Filtering to true top levels costs a references request per entity on each
  refresh.
* The entity picker and the hierarchy ask every VHDL file in the workspace for its symbols, so
  their cost grows with the number of files.
* Unused use clauses are found by looking up every identifier in the design unit, which
  over-reports a clause as used rather than wrongly calling it unused. Nothing is deleted without
  being selected first.
* Extract to constant or signal asks for the type, since the server cannot give the type of an
  arbitrary expression.
* A use clause added for a name used in an architecture goes above that architecture, which keeps
  it scoped to it. A `library` clause already given above its entity is therefore repeated, which
  is legal.
* When the server reports no symbols for a file, for example because it does not parse, the
  commands that need them say that no server answered.
* An instance generated from an entity maps each formal to an actual of the same name. Those
  actuals, and a generic map's values, are yours to adjust: an instance is not valid until the
  signals it names exist, which is what Declare Signals is for.
* No schematic or state diagram.

## Checking it

The generators and the grammar have unit checks that need nothing but Node:

```sh
cd editors/vscode
npm test
```

The commands themselves can only be checked in an editor, against a server. `npm run smoke` builds
a small two-library project, starts VS Code with the extension and a real `vhdl_ls`, and runs
every command and provider against it, answering their prompts. It asserts on the result, and on
whether the server accepts what was generated:

```sh
VHDL_LS=/path/to/vhdl_ls npm run smoke
```

It needs a display, the `code` command, and the VHDL-LS extension installed in that VS Code, so it
is run by hand rather than in CI.

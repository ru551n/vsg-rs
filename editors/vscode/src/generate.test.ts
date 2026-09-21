// Self-check: node --experimental-strip-types src/generate.test.ts
import assert from "node:assert/strict";
import {
  parseEntityHover,
  parseEnumHover,
  renderInstance,
  renderSignals,
  readActuals,
  renderFsm,
} from "./generate.ts";

// Verbatim textDocument/hover output from vhdl_ls (release build, probe3.mjs).
const ENTITY_HOVER = `entity leaf is
  generic (
    g_width : positive := 8
  );
  port (
    clk : in std_logic;
    din : in std_logic_vector(g_width - 1 downto 0);
    dout : out std_logic_vector(g_width - 1 downto 0)
  );
end entity;`;

const e = parseEntityHover(ENTITY_HOVER, "work");
assert.ok(e);
assert.equal(e.name, "leaf");
assert.equal(e.library, "work");
assert.deepEqual(e.generics.map((g) => g.name), ["g_width"]);
assert.equal(e.generics[0].def, "8");
assert.deepEqual(e.ports.map((p) => p.name), ["clk", "din", "dout"]);
assert.deepEqual(e.ports.map((p) => p.dir), ["in", "in", "out"]);
// The `downto 0)` inside the type must not end the port clause.
assert.equal(e.ports[1].type, "std_logic_vector(g_width - 1 downto 0)");

assert.equal(
  renderInstance(e),
  `  i_leaf : entity work.leaf
    generic map (
      g_width => g_width
    )
    port map (
      clk  => clk,
      din  => din,
      dout => dout
    );`,
);

// Ports already declared are skipped; generics are substituted into the type.
assert.equal(
  renderSignals(e.ports, {
    existing: ["clk"],
    genericValues: new Map([["g_width", "16"]]),
  }),
  `  signal din  : std_logic_vector(16 - 1 downto 0);
  signal dout : std_logic_vector(16 - 1 downto 0);`,
);

// Actuals are read out of the port map using the formals vhdl_ls reported.
const actuals = readActuals(
  "port map (clk => clk, din => data_in, dout => open)",
  e.ports.map((p) => p.name),
);
assert.equal(actuals.get("din"), "data_in");
assert.equal(
  renderSignals(e.ports, { existing: ["clk"], actuals }),
  "  signal data_in : std_logic_vector(g_width - 1 downto 0);",
);
// `open` is not an identifier to declare.
assert.ok(!renderSignals(e.ports, { actuals }).includes("open"));

const en = parseEnumHover("type state_t is (idle, run, done);");
assert.ok(en);
assert.deepEqual(en.literals, ["idle", "run", "done"]);
// A record or array type must not be mistaken for an enum.
assert.equal(parseEnumHover("type word_t is array (7 downto 0) of bit;"), null);

const fsm = renderFsm(en, { clock: "clk", reset: "rst" });
assert.ok(fsm.includes("signal state : state_t := idle;"));
assert.ok(fsm.includes("if rising_edge(clk) then"));
assert.ok(fsm.includes("when done =>"));
assert.ok(fsm.includes("if rst then"));

const async_ = renderFsm(en, { resetStyle: "async", reset: "rst_n" });
assert.ok(async_.includes("process (clk, rst_n) is"));
assert.ok(async_.includes("if rst_n then"));

console.log("ok");

// Snippet mode: the label is the first tab stop, then each actual in order.
const snip = renderInstance(e, { label: "u_leaf", snippet: true });
assert.ok(snip.includes("${1:u_leaf} : entity work.leaf"));
assert.ok(snip.includes("${2:g_width}"));
assert.ok(snip.includes("${3:clk}"));
assert.ok(snip.includes("${5:dout}"));
console.log("ok - snippet");

// --- context clause -------------------------------------------------------
import { contextClauseEdit, designatorOf } from "./generate.ts";

assert.equal(designatorOf("constant 'VitalDefaultPortFlag'"), "VitalDefaultPortFlag");
assert.equal(designatorOf("function COMPLEX_TO_POLAR[COMPLEX return COMPLEX_POLAR]"),
  "COMPLEX_TO_POLAR");
assert.equal(designatorOf("procedure stop[INTEGER]"), "stop");
assert.equal(designatorOf("array type 'VitalPortFlagVectorType'"), "VitalPortFlagVectorType");
// An operator is made visible by a use clause but is not named by one.
assert.equal(designatorOf('operator "/"[STD_ULOGIC_VECTOR, NATURAL return STD_ULOGIC_VECTOR]'),
  null);

// Nothing there yet: both clauses, with a blank line before the unit.
assert.deepEqual(
  contextClauseEdit(["entity foo is"], 0, "ieee", "numeric_std"),
  { line: 0, text: "library ieee;\nuse ieee.numeric_std.all;\n\n" },
);

// The library is already declared, so only the use clause is added, after the
// last existing clause rather than at the unit.
const existing = ["library ieee;", "use ieee.std_logic_1164.all;", "", "entity foo is"];
assert.deepEqual(
  contextClauseEdit(existing, 3, "ieee", "numeric_std"),
  { line: 2, text: "use ieee.numeric_std.all;\n" },
);

// Already visible.
assert.equal(contextClauseEdit(existing, 3, "ieee", "std_logic_1164"), null);

// work needs no library clause.
assert.deepEqual(
  contextClauseEdit(["entity foo is"], 0, "work", "my_pkg"),
  { line: 0, text: "use work.my_pkg.all;\n\n" },
);

// A second design unit later in the file gets its own clause, not the first one's.
const two = ["library ieee;", "use ieee.numeric_std.all;", "", "entity a is", "end entity;",
             "", "entity b is"];
assert.deepEqual(
  contextClauseEdit(two, 6, "ieee", "numeric_std"),
  { line: 6, text: "library ieee;\nuse ieee.numeric_std.all;\n\n" },
);

console.log("ok - context clause");

// --- associations, fill, component, context clause -------------------------
import {
  contextClause,
  missingFormals,
  readAssociations,
  renderComponent,
  renderMissingAssociations,
} from "./generate.ts";

const MAP = "port map (\n    clk => sys_clk,\n    din => data_in\n  );";
const assoc = readAssociations(MAP, e.ports.map((p) => p.name));
assert.deepEqual(assoc.map((a) => a.formal), ["clk", "din"]);
assert.deepEqual(assoc.map((a) => a.actual), ["sys_clk", "data_in"]);
// Offsets must point at the actual, so an inlay hint lands in the right place.
assert.equal(MAP.slice(assoc[1].start, assoc[1].end), "data_in");

const missing = missingFormals(e.ports, assoc.map((a) => a.formal));
assert.deepEqual(missing.map((p) => p.name), ["dout"]);
assert.equal(
  renderMissingAssociations(missing, "    ", true),
  ",\n    dout => dout",
);
assert.equal(renderMissingAssociations([], "    ", true), "");

assert.equal(
  renderComponent(e),
  `  component leaf is
    generic (
      g_width : positive := 8
    );
    port (
      clk  : in    std_logic;
      din  : in    std_logic_vector(g_width - 1 downto 0);
      dout : out   std_logic_vector(g_width - 1 downto 0)
    );
  end component;`,
);

const doc = [
  "library ieee;",
  "use ieee.std_logic_1164.all;",
  "use ieee.numeric_std.all;",
  "",
  "entity foo is",
];
const clauses = contextClause(doc, 4);
assert.deepEqual(clauses.map((c) => c.kind), ["library", "use", "use"]);
assert.deepEqual(clauses[0].names, ["ieee"]);
assert.deepEqual(clauses[2].names, ["ieee.numeric_std"]);
// A multi-name library clause lists each library.
assert.deepEqual(contextClause(["library ieee, work;", "entity f is"], 1)[0].names,
  ["ieee", "work"]);

console.log("ok - associations, component, context clause");

import { portMapShape } from "./generate.ts";

const FILLED = `u_x : entity work.leaf
  port map (
    clk => clk,
    din => din
  );`;
const shape = portMapShape(FILLED);
assert.ok(shape);
assert.equal(shape.hasEntries, true);
assert.equal(shape.indent, "    ");
// Appending here must land right after the last association, not after the newline.
assert.equal(FILLED.slice(0, shape.insertAt).endsWith("din => din"), true);

const empty = portMapShape("u_x : entity work.leaf port map ();");
assert.ok(empty);
assert.equal(empty.hasEntries, false);

// A parenthesised type in an actual must not be taken as the closing paren.
const nested = portMapShape("u_x : entity work.leaf\n  port map (\n    d => v(7 downto 0)\n  );");
assert.ok(nested);
assert.equal(nested.hasEntries, true);
assert.ok("u_x : entity work.leaf\n  port map (\n    d => v(7 downto 0)\n  );"
  .slice(0, nested.insertAt).endsWith("v(7 downto 0)"));

assert.equal(portMapShape("u_x : entity work.leaf;"), null);

console.log("ok - port map shape");

import { compareCandidates, compareLibraries } from "./generate.ts";

assert.deepEqual(
  ["work", "osvvm", "ieee", "std", "altera"].sort(compareLibraries),
  ["ieee", "altera", "osvvm", "std", "work"],
);
assert.deepEqual(
  [
    { library: "work", pkg: "types" },
    { library: "ieee", pkg: "numeric_std" },
    { library: "ieee", pkg: "math_real" },
    { library: "osvvm", pkg: "RandomPkg" },
  ].sort(compareCandidates).map((c) => `${c.library}.${c.pkg}`),
  ["ieee.math_real", "ieee.numeric_std", "osvvm.RandomPkg", "work.types"],
);
console.log("ok - library order");

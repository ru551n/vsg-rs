// Tokenize a VHDL sample with the same engine VS Code uses and assert scopes.
// Run: npm run test:grammar
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const oniguruma = require("vscode-oniguruma");
const textmate = require("vscode-textmate");
const wasm = readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm"));
await oniguruma.loadWASM(wasm.buffer);

const registry = new textmate.Registry({
  onigLib: Promise.resolve({
    createOnigScanner: (s) => new oniguruma.OnigScanner(s),
    createOnigString: (s) => new oniguruma.OnigString(s),
  }),
  loadGrammar: async () =>
    textmate.parseRawGrammar(
      readFileSync("syntaxes/vhdl.tmLanguage.json", "utf8"),
      "vhdl.tmLanguage.json",
    ),
});

const grammar = await registry.loadGrammar("source.vhdl");

const SAMPLE = [
  "-- a comment",
  "library ieee;",
  "entity leaf is",
  "  port (clk : in std_logic; q : out std_logic_vector(7 downto 0));",
  "end entity;",
  "architecture rtl of leaf is",
  "  type state_t is (idle, run);",
  "  constant c_mask : std_logic_vector(15 downto 0) := x\"00FF\";",
  "  constant c_base : integer := 16#FF#;",
  "begin",
  "  u_sub : entity work.other port map (clk => clk);",
  "  q <= (others => '0') when clk'event else \"1010\";",
  "end architecture;",
];

const scopesAt = [];
let rules = textmate.INITIAL;
for (const line of SAMPLE) {
  const r = grammar.tokenizeLine(line, rules);
  rules = r.ruleStack;
  for (const t of r.tokens)
    scopesAt.push({ text: line.slice(t.startIndex, t.endIndex), scopes: t.scopes });
}

/** Scopes of the first token whose text matches, ignoring surrounding space. */
const scopesOf = (text) =>
  scopesAt.find((t) => t.text.trim() === text.trim())?.scopes ?? [];
const has = (text, scope) =>
  scopesOf(text).some((s) => s.startsWith(scope));

const expect = [
  ["--", "comment.line", "comment delimiter"],
  ["a comment", "comment.line", "comment body"],
  ["entity", "keyword.other", "entity keyword"],
  ['x"00FF"', "constant.numeric.bit-string", "bit string literal"],
  ["16#FF#", "constant.numeric.based", "based literal"],
  ["'0'", "constant.character", "character literal"],
  ["event", "support.other.attribute", "attribute after tick"],
  ["<=", "keyword.operator.assignment", "signal assignment"],
  ["=>", "keyword.operator.assignment", "association"],
  ["downto", "keyword.other", "downto"],
  ["when", "keyword.control", "when"],
];

let bad = 0;
for (const [text, scope, what] of expect) {
  const ok = has(text, scope);
  if (!ok) {
    bad++;
    console.error(`FAIL ${what}: ${JSON.stringify(text)} -> ${JSON.stringify(scopesOf(text))}`);
  }
}
// Names are left to vhdl_ls's semantic tokens; the grammar must not claim them.
assert.ok(!has("leaf", "entity.name"), "grammar claims a design unit name");
assert.ok(!has("std_logic", "support.type"), "grammar claims a resolved type name");

// A bit-string literal must not be swallowed by the plain-string rule.
assert.ok(!has('x"00FF"', "string.quoted"), "bit string taken as a plain string");
// The tick in an attribute must not start a character literal.
assert.ok(!has("event", "constant.character"), "attribute taken as a character literal");

if (bad) {
  console.error(`\n${bad} scope expectation(s) failed`);
  process.exit(1);
}
console.log(`ok - ${expect.length} scopes`);

// Checks the generated colour themes: they match their palettes, are registered, are readable, and are credited.
// Run: npm run test
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { loadPalettes, render } from "./gen-themes.mjs";

const require = createRequire(import.meta.url);
const oniguruma = require("vscode-oniguruma");
const textmate = require("vscode-textmate");
const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const palettes = loadPalettes();
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const credits = readFileSync(join(root, "..", "..", "THIRD_PARTY_LICENSES.md"), "utf8");

const ROLES = [
  "bg", "bgDark", "bgLine", "bgSel", "fg", "fgDim", "gutter", "comment", "keyword", "operator",
  "string", "escape", "number", "constant", "type", "function", "property", "parameter",
  "namespace", "attribute", "error", "warning", "info", "added", "modified", "deleted", "accent",
];
const CODE = [
  "keyword", "operator", "string", "escape", "number", "constant", "type", "function", "property",
  "parameter", "namespace", "attribute",
];
const STATUS = ["error", "warning", "info"];

const luminance = (hex) => {
  const [r, g, b] = [1, 3, 5]
    .map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
    .map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const contrast = (a, b) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

await oniguruma.loadWASM(
  readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm")).buffer,
);
const grammarSource = readFileSync(join(root, "syntaxes", "vhdl.tmLanguage.json"), "utf8");

/** The colour a theme gives each piece of a VHDL sample, as VS Code's own tokenizer resolves it. */
async function coloursIn(theme) {
  const registry = new textmate.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: (s) => new oniguruma.OnigScanner(s),
      createOnigString: (s) => new oniguruma.OnigString(s),
    }),
    theme: { name: theme.name, settings: theme.tokenColors },
    loadGrammar: async () => textmate.parseRawGrammar(grammarSource, "vhdl.tmLanguage.json"),
  });
  const grammar = await registry.loadGrammar("source.vhdl");
  const map = registry.getColorMap();
  const seen = new Map();
  let stack = textmate.INITIAL;
  for (const line of SAMPLE) {
    const { tokens, ruleStack } = grammar.tokenizeLine2(line, stack);
    stack = ruleStack;
    for (let i = 0; i < tokens.length; i += 2) {
      const end = tokens[i + 2] ?? line.length;
      const text = line.slice(tokens[i], end).trim();
      const colour = map[(tokens[i + 1] & 0xff8000) >>> 15];
      if (text && !seen.has(text)) seen.set(text, colour?.toLowerCase());
    }
  }
  return seen;
}

const SAMPLE = [
  "-- a comment",
  "entity leaf is",
  "  constant c_mask : std_logic_vector(15 downto 0) := x\"00FF\";",
  "  constant c_base : integer := 16#FF#;",
  "begin",
  "  q <= (others => '0') when clk'event else \"1010\";",
];
// What each token is, in the roles a theme colours by.
const TOKENS = [
  ["-- a comment", "comment"],
  ["entity", "keyword"],
  ["downto", "keyword"],
  ["when", "keyword"],
  ["'0'", "string"],
  ['"1010"', "string"],
  ['x"00FF"', "number"],
  ["16#FF#", "number"],
  ["<=", "operator"],
  ["event", "attribute"],
];

const ids = palettes.map((p) => p.id);
assert.equal(new Set(ids).size, ids.length, "palette ids are unique");

for (const p of palettes) {
  const where = p.id;
  assert.deepEqual(Object.keys(p.roles).sort(), [...ROLES].sort(), `${where}: exactly the known roles`);
  for (const [role, value] of Object.entries(p.roles)) {
    assert.match(value, /^#[0-9a-f]{6}$/, `${where}.${role} is a lowercase #rrggbb`);
  }
  const { roles: r } = p;
  assert.ok(luminance(r.bg) < 0.05, `${where}: a dark theme has a dark background`);
  assert.ok(contrast(r.fg, r.bg) >= 4.5, `${where}: text on the background`);
  assert.ok(contrast(r.fg, r.bgSel) >= 3, `${where}: text on the selection`);
  assert.ok(contrast(r.comment, r.bg) >= 2, `${where}: comments are still legible`);
  for (const role of [...CODE, ...STATUS]) {
    assert.ok(contrast(r[role], r.bg) >= 3, `${where}.${role} on the background is ${contrast(r[role], r.bg).toFixed(2)}`);
  }

  const onDisk = readFileSync(join(root, "themes", `${p.id}.json`), "utf8");
  assert.equal(onDisk, render(p), `${where}: themes/${p.id}.json is stale, run npm run gen-themes`);
  assert.equal(JSON.parse(onDisk).type, "dark");

  const seen = await coloursIn(JSON.parse(onDisk));
  for (const [text, role] of TOKENS) {
    assert.equal(seen.get(text), r[role], `${where}: ${text} is coloured as ${role}`);
  }

  const entry = pkg.contributes.themes.find((t) => t.path === `./themes/${p.id}.json`);
  assert.ok(entry, `${where}: registered in package.json`);
  assert.equal(entry.uiTheme, "vs-dark");
  assert.equal(entry.label, p.name);

  assert.ok(credits.includes(`github.com/${p.source.repo})`), `${where}: ${p.source.repo} is credited`);
  assert.ok(credits.includes(p.source.holder), `${where}: ${p.source.holder} is credited`);
  assert.ok(credits.includes(p.name), `${where}: named in the credits`);
}

// Every theme file on disk is either a Gruvbox theme or generated.
const generated = new Set(palettes.map((p) => `${p.id}.json`));
for (const file of readdirSync(join(root, "themes"))) {
  assert.ok(generated.has(file) || file.startsWith("gruvbox-vhdl-"), `${file} has no palette`);
}
for (const t of pkg.contributes.themes) {
  const file = t.path.replace("./themes/", "");
  assert.ok(generated.has(file) || file.startsWith("gruvbox-vhdl-"), `${t.path} is not generated`);
  if (t.uiTheme === "vs") assert.ok(file.startsWith("gruvbox-vhdl-"), "only the Gruvbox theme is light");
}

console.log(`themes ok: ${palettes.length} generated`);

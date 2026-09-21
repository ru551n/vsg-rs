// Pure code generation. Input is text that vhdl_ls produced (hover contents),
// never raw user VHDL, so there is no parser here and none is wanted:
// vhdl_lang already did the analysis.

export interface Iface {
  name: string;
  dir?: string;
  type: string;
  def?: string;
}

export interface EntityIface {
  name: string;
  library: string;
  generics: Iface[];
  ports: Iface[];
}

export interface EnumType {
  name: string;
  literals: string[];
}

/** Text between the parens following `kw`, matched at depth. */
function clause(src: string, kw: string): string | null {
  const m = new RegExp(`\\b${kw}\\b\\s*\\(`, "i").exec(src);
  if (!m) return null;
  let depth = 0;
  const open = m.index + m[0].length - 1;
  for (let i = open; i < src.length; i++) {
    if (src[i] === "(") depth++;
    else if (src[i] === ")" && --depth === 0) return src.slice(open + 1, i);
  }
  return null;
}

/** Split on `sep` at paren depth zero. */
function splitTop(text: string, sep: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let last = 0;
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "(") depth++;
    else if (text[i] === ")") depth--;
    else if (text[i] === sep && depth === 0) {
      out.push(text.slice(last, i));
      last = i + 1;
    }
  }
  out.push(text.slice(last));
  return out.map((s) => s.trim()).filter(Boolean);
}

function parseIface(text: string): Iface[] {
  return splitTop(text, ";").map((item) => {
    const eq = item.indexOf(":=");
    const def = eq >= 0 ? item.slice(eq + 2).trim() : undefined;
    const lhs = (eq >= 0 ? item.slice(0, eq) : item).trim();
    const colon = lhs.indexOf(":");
    const name = lhs
      .slice(0, colon)
      .replace(/^\s*(signal|constant|variable)\s+/i, "")
      .trim();
    let rest = lhs.slice(colon + 1).trim();
    const dm = /^(in|out|inout|buffer|linkage)\b\s*/i.exec(rest);
    if (dm) rest = rest.slice(dm[0].length);
    return {
      name,
      dir: dm ? dm[1].toLowerCase() : undefined,
      type: rest.replace(/\s+/g, " ").trim(),
      def,
    };
  }).filter((i) => i.name && i.type);
}

/**
 * Parse the entity declaration that `textDocument/hover` returns.
 * `library` comes from the workspace symbol's containerName.
 */
export function parseEntityHover(hover: string, library = "work"): EntityIface | null {
  const m = /\bentity\s+(\w+)\s+is\b/i.exec(hover);
  if (!m) return null;
  const g = clause(hover, "generic");
  const p = clause(hover, "port");
  return {
    name: m[1],
    library,
    generics: g ? parseIface(g) : [],
    ports: p ? parseIface(p) : [],
  };
}

/** Parse `type state_t is (idle, run, done);` as hover returns it. */
export function parseEnumHover(hover: string): EnumType | null {
  const m = /\btype\s+(\w+)\s+is\s*\(/i.exec(hover);
  if (!m) return null;
  const body = clause(hover.slice(m.index), "is");
  if (body === null) return null;
  const literals = splitTop(body, ",").map((s) => s.trim()).filter(Boolean);
  if (!literals.length || literals.some((l) => !/^('.'|\w+)$/.test(l))) return null;
  return { name: m[1], literals };
}

const pad = (xs: string[]) => Math.max(0, ...xs.map((x) => x.length));

/** Actuals that look like identifiers but never name a signal. */
const NOT_A_SIGNAL = new Set(["open", "others", "null", "unaffected", "inertial"]);

function assocList(items: Iface[], actual: (i: Iface) => string, indent: string): string[] {
  const w = pad(items.map((i) => i.name));
  return items.map(
    (i, n) => `${indent}${i.name.padEnd(w)} => ${actual(i)}${n < items.length - 1 ? "," : ""}`,
  );
}

export interface InstanceOptions {
  label?: string;
  indent?: string;
  /** Omit generics left at their default value. */
  skipDefaultedGenerics?: boolean;
  /** Emit LSP snippet placeholders so the actuals can be tabbed through. */
  snippet?: boolean;
  /**
   * How the library is spelled. Defaults to the one the server reported, which is only right
   * when the file declares it: inside the library the file is itself analysed in, `work` is
   * the name that needs no clause, and spelling that library out without one is an error.
   */
  library?: string;
}

/** Instantiation of an entity, formals mapped to like-named actuals. */
export function renderInstance(e: EntityIface, opts: InstanceOptions = {}): string {
  const i = opts.indent ?? "  ";
  const label = opts.label ?? `i_${e.name}`;
  const generics = opts.skipDefaultedGenerics
    ? e.generics.filter((g) => g.def === undefined)
    : e.generics;
  let stop = 0;
  const actual = (x: Iface) =>
    opts.snippet ? `\${${++stop}:${x.name}}` : x.name;

  const head = opts.snippet ? `\${${++stop}:${label}}` : label;
  const out = [`${i}${head} : entity ${opts.library ?? e.library}.${e.name}`];
  if (generics.length) {
    out.push(`${i}  generic map (`);
    out.push(...assocList(generics, actual, `${i}    `));
    out.push(`${i}  )`);
  }
  if (e.ports.length) {
    out.push(`${i}  port map (`);
    out.push(...assocList(e.ports, actual, `${i}    `));
    out.push(`${i}  );`);
  } else {
    out[out.length - 1] += ";";
  }
  return out.join("\n");
}

/** Replace generic names in a type mark with the values used in the generic map. */
export function substituteGenerics(type: string, values: Map<string, string>): string {
  let out = type;
  for (const [name, value] of values)
    out = out.replace(new RegExp(`\\b${name}\\b`, "gi"), value);
  return out;
}

export interface SignalOptions {
  indent?: string;
  /** Names that already have a declaration, from documentSymbol. */
  existing?: Iterable<string>;
  /** formal -> actual, read out of the port map. */
  actuals?: Map<string, string>;
  /** generic -> value, for types like std_logic_vector(g_width - 1 downto 0). */
  genericValues?: Map<string, string>;
}

/**
 * Signal declarations for every port of an instantiation that is not declared yet.
 * ponytail: types are copied verbatim from the port, with generic names textually
 * substituted. A port typed by an unconstrained array resolved through the actual
 * needs elaboration, which vhdl_ls does not expose; such a type lands as written.
 */
export function renderSignals(ports: Iface[], opts: SignalOptions = {}): string {
  const indent = opts.indent ?? "  ";
  const declared = new Set(
    [...(opts.existing ?? [])].map((s) => s.toLowerCase()),
  );
  const generics = opts.genericValues ?? new Map();
  const seen = new Set<string>();
  const rows: [string, string][] = [];

  for (const p of ports) {
    const actual = opts.actuals?.get(p.name.toLowerCase()) ?? p.name;
    const key = actual.toLowerCase();
    // An actual that is an expression, a slice or `open` is not a signal to declare.
    if (!/^[a-z]\w*$/i.test(actual) || NOT_A_SIGNAL.has(actual.toLowerCase())) continue;
    if (declared.has(key) || seen.has(key)) continue;
    seen.add(key);
    rows.push([actual, substituteGenerics(p.type, generics)]);
  }

  if (!rows.length) return "";
  const w = pad(rows.map(([n]) => n));
  return rows.map(([n, t]) => `${indent}signal ${n.padEnd(w)} : ${t};`).join("\n");
}

/** formal => actual pairs of a port map, keyed by the formals vhdl_ls reported. */
export function readActuals(portMapText: string, formals: string[]): Map<string, string> {
  const out = new Map<string, string>();
  for (const f of formals) {
    const m = new RegExp(`\\b${f}\\s*=>\\s*([^,)]+)`, "i").exec(portMapText);
    if (m) out.set(f.toLowerCase(), m[1].trim());
  }
  return out;
}

export interface FsmOptions {
  indent?: string;
  /** Indentation of the process, when it is not written beside the declaration. */
  processIndent?: string;
  signal?: string;
  clock?: string;
  reset?: string;
  resetStyle?: "sync" | "async" | "none";
}

/**
 * A registered state machine over an existing enum type, in the two places VHDL wants it.
 *
 * The state signal is a declaration and the process is a concurrent statement, and an
 * architecture keeps them either side of `begin`. Written together, as one block, the process
 * sits among declarations, which is an error.
 */
export function renderFsmParts(
  e: EnumType,
  opts: FsmOptions = {},
): { declaration: string; process: string } {
  const i = opts.indent ?? "  ";
  const p = opts.processIndent ?? i;
  const sig = opts.signal ?? "state";
  const clk = opts.clock ?? "clk";
  const rst = opts.reset ?? "reset";
  const style = opts.resetStyle ?? "sync";
  const idle = e.literals[0];
  const body: string[] = [];

  body.push(
    `${p}p_${sig} : process (${clk}${style === "async" ? `, ${rst}` : ""}) is`,
  );
  body.push(`${p}begin`);

  const inner: string[] = [];
  inner.push(`${p}    case ${sig} is`);
  for (const lit of e.literals) {
    inner.push(`${p}      when ${lit} =>`);
    inner.push(`${p}        null;`);
    inner.push("");
  }
  inner.pop();
  inner.push(`${p}    end case;`);

  if (style === "async") {
    body.push(`${p}  if ${rst} then`);
    body.push(`${p}    ${sig} <= ${idle};`);
    body.push(`${p}  elsif rising_edge(${clk}) then`);
    body.push(...inner.map((l) => l.replace(/^ {2}/, "")));
    body.push(`${p}  end if;`);
  } else {
    body.push(`${p}  if rising_edge(${clk}) then`);
    body.push(...inner.map((l) => l.replace(/^ {2}/, "")));
    if (style === "sync") {
      body.push("");
      body.push(`${p}    if ${rst} then`);
      body.push(`${p}      ${sig} <= ${idle};`);
      body.push(`${p}    end if;`);
    }
    body.push(`${p}  end if;`);
  }

  body.push(`${p}end process;`);
  return {
    declaration: `${i}signal ${sig} : ${e.name} := ${idle};`,
    process: body.join("\n"),
  };
}

/** The same, as one block, for a caller that places it itself. */
export function renderFsm(e: EnumType, opts: FsmOptions = {}): string {
  const { declaration, process } = renderFsmParts(e, opts);
  return `${declaration}\n\n${process}`;
}

/** Libraries that are visible without a library clause. */
const IMPLICIT_LIBRARIES = new Set(["work", "std"]);

/**
 * The identifier a workspace symbol declares, as vhdl_ls names it:
 * `constant 'c_foo'`, `function to_integer[...]`. Operator symbols return null,
 * since they are made visible by a use clause but not named by one.
 */
export function designatorOf(symbolName: string): string | null {
  const quoted = /'([^']+)'/.exec(symbolName);
  if (quoted) return quoted[1];
  const subprogram = /\b([A-Za-z]\w*)\s*\[/.exec(symbolName);
  return subprogram ? subprogram[1] : null;
}

export interface ContextEdit {
  /** Line to insert before. */
  line: number;
  text: string;
}

/**
 * The context clause needed to make `library.pkg` visible to the design unit
 * starting at `unitLine`, or null when it already is. Without `pkg` only the library
 * itself is wanted, as for `entity lib.name`, and only its clause is added.
 *
 * ponytail: the existing clause is recognised by matching `library` and `use`
 * at the start of a line. A clause split across lines is not detected and would
 * produce a duplicate, which is legal VHDL and flagged by the server.
 */
export function contextClauseEdit(
  lines: string[],
  unitLine: number,
  library: string,
  pkg?: string,
): ContextEdit | null {
  const lib = library.toLowerCase();

  let start = unitLine;
  while (
    start > 0 &&
    /^\s*(library\b|use\b|--|\s*$)/i.test(lines[start - 1])
  )
    start--;

  const region = lines.slice(start, unitLine);
  if (pkg) {
    const isUse = new RegExp(`^\\s*use\\s+${lib}\\s*\\.\\s*${pkg}\\s*\\.`, "i");
    if (region.some((l) => isUse.test(l))) return null;
  }

  const hasLibrary =
    IMPLICIT_LIBRARIES.has(lib) ||
    region.some((l) => new RegExp(`^\\s*library\\b[^;]*\\b${lib}\\b`, "i").test(l));

  if (!pkg && hasLibrary) return null;

  let lastClause = -1;
  region.forEach((l, i) => {
    if (/^\s*(library|use)\b/i.test(l)) lastClause = i;
  });

  const line = lastClause >= 0 ? start + lastClause + 1 : unitLine;
  const indent = /^\s*/.exec(lines[unitLine] ?? "")![0];

  let text = "";
  if (!hasLibrary) text += `${indent}library ${library};\n`;
  if (pkg) text += `${indent}use ${library}.${pkg}.all;\n`;
  // Keep a blank line between the clause and the design unit it precedes.
  if (line === unitLine && (lines[unitLine] ?? "").trim()) text += "\n";

  return { line, text };
}

export interface Association {
  formal: string;
  actual: string;
  /** Offsets of the actual within the text that was searched. */
  start: number;
  end: number;
}

/**
 * `formal => actual` pairs, located by the formal names the server reported.
 * ponytail: named association only. Positional association is not recognised,
 * and a formal appearing inside an actual expression could mislead the search.
 */
export function readAssociations(text: string, formals: string[]): Association[] {
  const out: Association[] = [];
  for (const f of formals) {
    const m = new RegExp(`\\b${f}\\s*=>\\s*([^,)]+)`, "i").exec(text);
    if (!m) continue;
    const actual = m[1].trimEnd();
    const start = m.index + m[0].length - m[1].length;
    out.push({ formal: f, actual: actual.trim(), start, end: start + actual.length });
  }
  return out.sort((a, b) => a.start - b.start);
}

/** Formals of `ports` that the port map does not associate. */
export function missingFormals(ports: Iface[], associated: Iterable<string>): Iface[] {
  const have = new Set([...associated].map((s) => s.toLowerCase()));
  return ports.filter((p) => !have.has(p.name.toLowerCase()));
}

/**
 * Association lines to append to an existing map. `precededByEntries` decides
 * whether the block has to start with a comma continuing the previous entry.
 */
export function renderMissingAssociations(
  missing: Iface[],
  indent: string,
  precededByEntries: boolean,
): string {
  if (!missing.length) return "";
  const w = Math.max(0, ...missing.map((p) => p.name.length));
  const rows = missing.map((p) => `${indent}${p.name.padEnd(w)} => ${p.name}`);
  return (precededByEntries ? ",\n" : "") + rows.join(",\n");
}

/** Component declaration matching an entity, for old style instantiation. */
export function renderComponent(e: EntityIface, indent = "  "): string {
  const out = [`${indent}component ${e.name} is`];
  const clause = (kw: string, items: Iface[], close: string) => {
    if (!items.length) return;
    const w = Math.max(0, ...items.map((i) => i.name.length));
    out.push(`${indent}  ${kw} (`);
    items.forEach((i, n) => {
      const dir = i.dir ? `${i.dir.padEnd(5)} ` : "";
      const def = i.def !== undefined ? ` := ${i.def}` : "";
      out.push(
        `${indent}    ${i.name.padEnd(w)} : ${dir}${i.type}${def}${n < items.length - 1 ? ";" : ""}`,
      );
    });
    out.push(`${indent}  )${close}`);
  };
  clause("generic", e.generics, ";");
  clause("port", e.ports, ";");
  out.push(`${indent}end component;`);
  return out.join("\n");
}

export interface ClauseLine {
  index: number;
  kind: "library" | "use";
  /** Lower-cased names the clause makes visible. */
  names: string[];
}

/** The context clause preceding the design unit at `unitLine`. */
export function contextClause(lines: string[], unitLine: number): ClauseLine[] {
  let start = unitLine;
  while (start > 0 && /^\s*(library\b|use\b|--|\s*$)/i.test(lines[start - 1])) start--;

  const out: ClauseLine[] = [];
  for (let i = start; i < unitLine; i++) {
    const lib = /^\s*library\s+([^;]+);/i.exec(lines[i]);
    if (lib) {
      out.push({
        index: i,
        kind: "library",
        names: lib[1].split(",").map((s) => s.trim().toLowerCase()).filter(Boolean),
      });
      continue;
    }
    const use = /^\s*use\s+([^;]+);/i.exec(lines[i]);
    if (use) {
      out.push({
        index: i,
        kind: "use",
        names: use[1]
          .split(",")
          .map((s) => s.trim().split(".").slice(0, 2).join(".").toLowerCase())
          .filter(Boolean),
      });
    }
  }
  return out;
}

export interface PortMapShape {
  /** Offset just after the last association, where new ones are appended. */
  insertAt: number;
  /** Whether the map already has associations to continue with a comma. */
  hasEntries: boolean;
  /** Indent of the last association line, reused for the new ones. */
  indent: string;
}

/** Locate where to append associations in an existing `port map (...)`. */
export function portMapShape(text: string): PortMapShape | null {
  const kw = /\bport\s+map\b/i.exec(text);
  if (!kw) return null;
  let i = text.indexOf("(", kw.index + kw[0].length);
  if (i < 0) return null;

  const open = i;
  let depth = 0;
  let close = -1;
  for (; i < text.length; i++) {
    if (text[i] === "(") depth++;
    else if (text[i] === ")" && --depth === 0) {
      close = i;
      break;
    }
  }
  if (close < 0) return null;

  const body = text.slice(open + 1, close);
  const hasEntries = body.trim().length > 0;
  let insertAt = close;
  while (insertAt > open + 1 && /\s/.test(text[insertAt - 1])) insertAt--;

  const lastLine = text.lastIndexOf("\n", insertAt - 1);
  const indent = hasEntries && lastLine >= 0
    ? /^\s*/.exec(text.slice(lastLine + 1))![0]
    : "    ";
  return { insertAt, hasEntries, indent };
}

/** ieee first, then everything else alphabetically, then work. */
export function compareLibraries(a: string, b: string): number {
  const rank = (lib: string) => {
    const l = lib.toLowerCase();
    if (l === "ieee") return 0;
    if (l === "work") return 2;
    return 1;
  };
  return rank(a) - rank(b) || a.toLowerCase().localeCompare(b.toLowerCase());
}

/** Order candidate packages for any list shown to the user. */
export function compareCandidates(
  a: { library: string; pkg: string },
  b: { library: string; pkg: string },
): number {
  return (
    compareLibraries(a.library, b.library) ||
    a.pkg.toLowerCase().localeCompare(b.pkg.toLowerCase())
  );
}

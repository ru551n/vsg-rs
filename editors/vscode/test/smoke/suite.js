// The editing features, run inside a real extension host against a real VHDL-LS.
//
// Everything in `src/editing.ts` talks to a language server through VS Code, so none of it can
// be checked by a unit test: the only honest check is to start the editor and ask. This runs
// there, in the same API scope as the extension under test, which is what lets it answer the
// prompts the commands raise. It is started by `run.mjs`, not by hand.
//
// Each step is time-boxed and reported the moment it finishes, so a step that hangs still leaves
// evidence of how far the run got.

const vscode = require("vscode");
const fs = require("fs");
const path = require("path");

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const lines = [];
const out = (line) => {
  lines.push(line);
  fs.writeFileSync(process.env.VSGRS_RESULTS, lines.join("\n") + "\n");
};
const check = (ok, name, detail) =>
  out(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? "  --  " + detail : ""}`);

const limit = (promise, ms, what) =>
  Promise.race([
    Promise.resolve(promise).catch((error) => {
      throw new Error(`${what}: ${error.message}`);
    }),
    wait(ms).then(() => {
      throw new Error(`timed out after ${ms}ms: ${what}`);
    }),
  ]);

async function until(probe, ms, what) {
  const end = Date.now() + ms;
  for (;;) {
    const value = await probe();
    if (value) return value;
    if (Date.now() > end) throw new Error(`gave up waiting for ${what}`);
    await wait(400);
  }
}

exports.run = async function run() {
  try {
    const folder = vscode.workspace.workspaceFolders[0].uri.fsPath;
    const at = (name) => vscode.Uri.file(path.join(folder, name));
    const top = at("top.vhd");
    const doc = await vscode.workspace.openTextDocument(top);
    const editor = await vscode.window.showTextDocument(doc);

    const symbols = (query) =>
      vscode.commands.executeCommand("vscode.executeWorkspaceSymbolProvider", query);
    const documentSymbols = async (uri) =>
      (await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", uri)) || [];
    const hintsIn = (uri, document) =>
      vscode.commands.executeCommand(
        "vscode.executeInlayHintProvider",
        uri,
        new vscode.Range(new vscode.Position(0, 0), new vscode.Position(document.lineCount, 0)),
      );
    const instancesIn = async (uri) =>
      (await documentSymbols(uri))
        .flatMap((symbol) => symbol.children || [])
        .filter((symbol) => /^instance/.test(symbol.name));
    // What must hold of a generated instance is that its references resolve. Actuals naming
    // signals that do not exist yet are expected until they are declared.
    const unresolved = () =>
      vscode.languages
        .getDiagnostics(top)
        .filter((d) => /No declaration of '(mylib|other|work|fifo|leaf)'|No primary unit/i.test(d.message));

    // Gate on the project's own entities. `fifo` also fuzzy-matches library symbols such as
    // find_leftmost, so accepting any result would pass before the project had been read at all.
    const started = Date.now();
    await until(
      async () =>
        (await symbols("fifo")).some((s) => /^entity 'fifo'/i.test(s.name) && s.containerName === "mylib") &&
        (await symbols("leaf")).some((s) => /^entity 'leaf'/i.test(s.name) && s.containerName === "other"),
      90000,
      "VHDL-LS to analyse both libraries",
    );
    out(`server ready after ${Date.now() - started}ms`);

    // The prompts, answered as a user accepting the defaults would.
    let pick = "fifo";
    let offered = 0;
    vscode.window.showQuickPick = async (items) => {
      const list = await items;
      offered = list.length;
      return list.find((item) => item.label === pick) || list[0];
    };
    vscode.window.showInputBox = async (options) => options && options.value;

    // A blank, indented line before `marker`, as auto-indent leaves one.
    const openLineBefore = async (marker) => {
      const line = doc.getText().split("\n").findIndex((text) => text.startsWith(marker));
      await editor.edit((edit) => edit.insert(new vscode.Position(line, 0), "  \n"));
      const cursor = new vscode.Position(line, 2);
      editor.selection = new vscode.Selection(cursor, cursor);
    };

    // 1. Inlay hints on a port map that was already there.
    const first = await limit(hintsIn(top, doc), 20000, "inlay hints");
    check(
      first.length === 2 && first.every((h) => /in std_logic/.test(String(h.label))),
      "inlay hints on an existing port map",
      `${first.length}: ${first.map((h) => h.label).join(" / ")}`,
    );

    // 2. Instantiate an entity from the library the file is itself analysed in: `work`, and
    // nothing to add. Spelling the library out would need a clause the file does not have.
    await openLineBefore("  u_fifo");
    const before = doc.getText();
    await limit(vscode.commands.executeCommand("vsg-rs.instantiateEntity"), 30000, "instantiateEntity");
    const after = doc.getText();
    check(
      /i_fifo : entity work\.fifo/.test(after) &&
        /generic map/.test(after) &&
        /port map/.test(after) &&
        !/library mylib/.test(after),
      "instantiate from the file's own library",
      `${after.length - before.length} chars, spelled work.fifo, no library clause`,
    );
    check(
      offered === 43,
      "the picker offers every entity, past the server's 200-symbol cap",
      `${offered} of 43`,
    );

    await until(async () => (await instancesIn(top)).length === 2, 30000, "the server to see the new instance");
    check(
      unresolved().length === 0,
      "the generated instance names a library and entity that resolve",
      unresolved().map((d) => d.message).join(" | ") || "nothing unresolved",
    );

    // 3. Give the generic a value, as a user would, then declare the signals the map needs.
    const generic = doc.getText().indexOf("width => width");
    await editor.edit((edit) =>
      edit.replace(
        new vscode.Range(doc.positionAt(generic), doc.positionAt(generic + "width => width".length)),
        "width => 16",
      ),
    );
    await until(async () => (await instancesIn(top)).length === 2, 30000, "the server to see the edit");
    const inside = doc.positionAt(doc.getText().indexOf("i_fifo") + 3);
    editor.selection = new vscode.Selection(inside, inside);
    await limit(vscode.commands.executeCommand("vsg-rs.declareSignals"), 30000, "declareSignals");
    const declared = doc.getText();
    const names = ["din", "dout", "empty"].filter((n) => new RegExp(`signal\\s+${n}\\s*:`).test(declared));
    check(names.length === 3, "declare signals for a port map", `declared: ${names.join(",")}`);
    check(
      /signal\s+din\s*:\s*std_logic_vector\(16 - 1 downto 0\)/.test(declared),
      "the generic's value is substituted into a declared type",
    );
    check((declared.match(/signal\s+clk\s*:/g) || []).length === 1, "signals that exist are not redeclared");

    // 4. An entity from ANOTHER library is only visible after a library clause.
    pick = "leaf";
    await openLineBefore("end architecture");
    await limit(vscode.commands.executeCommand("vsg-rs.instantiateEntity"), 30000, "instantiateEntity (leaf)");
    const crossed = doc.getText();
    check(/i_leaf : entity other\.leaf/.test(crossed), "an entity from another library is named by it");
    check(/^library other;$/m.test(crossed), "and the library clause it needs is added");
    await until(async () => (await instancesIn(top)).length === 3, 30000, "the server to see the third instance");
    await wait(1500);
    check(
      unresolved().length === 0,
      "the cross-library instance resolves",
      unresolved().map((d) => `line ${d.range.start.line + 1}: ${d.message}`).join(" | ") || "nothing unresolved",
    );

    // 5. Inlay hints on every instance.
    const every = await limit(hintsIn(top, doc), 20000, "inlay hints after edits");
    check(every.length >= 2 + 6 + 2, "inlay hints on every instance", `${every.length} hints`);

    // 6. The design hierarchy.
    const { DesignHierarchy } = require(path.join(process.env.VSGRS_EXT, "out", "editing.js"));
    const tree = new DesignHierarchy();
    const roots = await limit(tree.getChildren(), 30000, "hierarchy roots");
    const listed = roots.map((r) => `${r.description}.${r.label}`);
    check(
      listed.includes("mylib.top") && listed.includes("other.leaf") && roots.length === 43,
      "the hierarchy lists every entity, across libraries",
      `${roots.length} of 43`,
    );
    const topNode = roots.find((r) => r.label === "top");
    const children = topNode ? await limit(tree.getChildren(topNode), 30000, "children of top") : [];
    const resolved = children.map((c) => `${c.label}:${c.description}`).sort().join(", ");
    check(
      resolved === "i_fifo:mylib.fifo, i_leaf:other.leaf, u_fifo:mylib.fifo",
      "the hierarchy resolves every instance to its entity",
      resolved,
    );
    if (children.length) {
      check(tree.getTreeItem(children[0]).command.command === "vscode.open", "a tree item opens its source");
    }
    await vscode.commands.executeCommand("vsg-rs.hierarchy.focus");
    check(true, "the view can be focused");

    // 7. Quick fixes: the one that maps missing ports, and the wiring of the extract commands.
    const onInstance = doc.positionAt(doc.getText().indexOf("u_fifo") + 2);
    const fixes = await vscode.commands.executeCommand(
      "vscode.executeCodeActionProvider",
      top,
      new vscode.Range(onInstance, onInstance),
    );
    const map = fixes.find((a) => /^Map \d+ missing ports?$/.test(a.title));
    check(!!map, "a quick fix maps the ports an instance leaves out", map ? map.title : "not offered");
    if (map) {
      await vscode.workspace.applyEdit(map.edit);
      const block = doc.getText().slice(doc.getText().indexOf("u_fifo"));
      check(/din\s*=>/.test(block.split("end architecture")[0]), "and writes the missing formals");
    }
    const sixteen = doc.getText().indexOf("width => 16") + "width => ".length;
    const selected = new vscode.Range(doc.positionAt(sixteen), doc.positionAt(sixteen + 2));
    const refactors = await vscode.commands.executeCommand("vscode.executeCodeActionProvider", top, selected);
    const extract = refactors.find((a) => a.title === "Extract to constant");
    check(
      !!extract && extract.command.command === "vsg-rs.extractObject" && extract.command.arguments[0].fsPath === top.fsPath,
      "the extract action carries its document, under the vsg-rs command name",
    );

    // Extract with ANOTHER document focused. The action names its document, so the edit must
    // land there; falling back to the active editor would rewrite whichever file has focus.
    const fifo = await vscode.workspace.openTextDocument(at("fifo.vhd"));
    await vscode.window.showTextDocument(fifo);
    const fifoBefore = fifo.getText();
    await limit(
      vscode.commands.executeCommand("vsg-rs.extractObject", top, selected, "constant"),
      20000,
      "extractObject",
    );
    const extracted = doc.getText();
    check(
      /constant c_value\s*:/.test(extracted) && /width => c_value/.test(extracted),
      "extract edits the document the action came from",
    );
    check(fifo.getText() === fifoBefore && !fifo.isDirty, "and leaves the focused document alone");

    // 8. A resolution that failed because the server did not yet know the entity must not be
    // remembered. The document below never changes, so its version never does either: a cached
    // failure would keep it hint-less for good.
    const create = async (name, text) => {
      const edit = new vscode.WorkspaceEdit();
      edit.createFile(at(name), { overwrite: true });
      edit.insert(at(name), new vscode.Position(0, 0), text);
      await vscode.workspace.applyEdit(edit);
      const created = await vscode.workspace.openTextDocument(at(name));
      await created.save();
      return created;
    };
    const later = await create(
      "unit98.vhd",
      "library ieee;\nuse ieee.std_logic_1164.all;\n\nentity unit98 is\nend entity unit98;\n\n" +
        "architecture rtl of unit98 is\nbegin\n\n  u : entity work.unit99\n    port map (\n" +
        "      p0 => open\n    );\n\nend architecture rtl;\n",
    );
    await until(async () => (await instancesIn(later.uri)).length === 1, 30000, "the server to read unit98");
    const unknown = await limit(hintsIn(later.uri, later), 20000, "hints before the entity exists");
    check(unknown.length === 0, "no hints while the entity is unknown", `${unknown.length}`);
    await create(
      "unit99.vhd",
      "library ieee;\nuse ieee.std_logic_1164.all;\n\nentity unit99 is\n  port (\n    p0 : in std_logic\n  );\nend entity unit99;\n",
    );
    await until(
      async () => (await symbols("unit99")).some((s) => /^entity 'unit99'/i.test(s.name)),
      30000,
      "the server to read unit99",
    );
    const known = await limit(hintsIn(later.uri, later), 20000, "hints once the entity exists");
    check(
      known.length === 1,
      "hints appear once the entity exists, without the document changing",
      `${known.length}`,
    );

    out("done");
  } catch (error) {
    out("HARNESS ERROR: " + (error && error.stack ? error.stack : error));
    out("done");
  }
};

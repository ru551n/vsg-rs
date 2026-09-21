// Run the editing features in a real VS Code, against a real VHDL-LS.
//
//   VHDL_LS=/path/to/vhdl_ls npm run smoke
//
// The editing half of this extension asks a VHDL language server everything it writes, so it can
// only be checked by starting an editor and a server. This builds a small project (two
// libraries, and enough entities to pass the server's 200-symbol cap on a workspace query),
// copies the compiled extension beside a suite, launches VS Code with both, and reports what
// the suite found.
//
// It needs a display, the `code` command, the VHDL-LS extension installed in that VS Code, and a
// `vhdl_ls` binary that can find its `vhdl_libraries`. It is not part of CI for those reasons.
//
//   VHDL_LS   the vhdl_ls executable (required)
//   CODE      the VS Code command (default: code)
//   KEEP=1    keep the scratch directory after a passing run

import { execFileSync, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..", "..");
const vhdlLs = process.env.VHDL_LS;
const code = process.env.CODE ?? "code";

if (!vhdlLs || !existsSync(vhdlLs)) {
  console.error("Set VHDL_LS to a vhdl_ls executable that can find its vhdl_libraries.");
  process.exit(2);
}
if (!existsSync(join(root, "out", "editing.js"))) {
  console.error("Run `npm run compile` first: the extension under test is out/.");
  process.exit(2);
}

const scratch = mkdtempSync(join(process.env.TMPDIR ?? tmpdir(), "vsg-rs-smoke-"));
const workspace = join(scratch, "ws");
const extension = join(scratch, "ext");
const userData = join(scratch, "profile");
const results = join(scratch, "results.txt");
for (const directory of [workspace, join(workspace, "other"), join(workspace, ".vscode"), extension, userData]) {
  mkdirSync(directory, { recursive: true });
}
const write = (name, text) => writeFileSync(join(workspace, name), text);

// top, fifo, leaf, fsm, usage and clauses, and the forty below.
const ENTITIES = 46;

// Two libraries. A library name of `work` in vhdl_ls.toml is silently ignored by the server, so
// neither uses it.
write("vhdl_ls.toml", "[libraries]\nmylib.files = ['*.vhd']\nother.files = ['other/*.vhd']\n");
write(".vscode/settings.json", JSON.stringify({ "vhdlls.languageServer": "user", "vhdlls.languageServerUserPath": vhdlLs }, null, 2));

write("fifo.vhd", `library ieee;
use ieee.std_logic_1164.all;

entity fifo is
  generic (
    width : positive := 8
  );
  port (
    clk   : in    std_logic;
    rst   : in    std_logic;
    din   : in    std_logic_vector(width - 1 downto 0);
    dout  : out   std_logic_vector(width - 1 downto 0);
    empty : out   std_logic
  );
end entity fifo;

architecture rtl of fifo is

begin

end architecture rtl;
`);

write("top.vhd", `library ieee;
use ieee.std_logic_1164.all;

entity top is
end entity top;

architecture rtl of top is

  signal clk : std_logic;
  signal rst : std_logic;

begin

  u_fifo : entity work.fifo
    port map (
      clk => clk,
      rst => rst
    );

end architecture rtl;
`);

write("other/leaf.vhd", `library ieee;
use ieee.std_logic_1164.all;

entity leaf is
  port (
    a : in  std_logic;
    y : out std_logic
  );
end entity leaf;

architecture rtl of leaf is
begin
  y <= a;
end architecture rtl;
`);

// A state machine over an enumeration type that spans several lines, so the signal has to go
// after all of it and the process after the architecture's own `begin`.
write("fsm.vhd", `library ieee;
use ieee.std_logic_1164.all;

entity fsm is
  port (
    clk : in std_logic;
    rst : in std_logic;
    go  : in std_logic
  );
end entity fsm;

architecture rtl of fsm is

  type state_t is (
    idle,
    run,
    finish
  );

begin

end architecture rtl;
`);

// A use clause that nothing needs, beside one that is needed.
write("clauses.vhd", `library ieee;
use ieee.std_logic_1164.all;
use ieee.math_real.all;

entity clauses is
  port (
    a : in std_logic
  );
end entity clauses;
`);

// A type the file cannot see yet: it needs \`use ieee.numeric_std.all\`.
write("usage.vhd", `library ieee;
use ieee.std_logic_1164.all;

entity usage is
end entity usage;

architecture rtl of usage is

  signal x : unsigned(3 downto 0);

begin

end architecture rtl;
`);

// Forty more, with eight ports each. The server answers a workspace symbol query with at most
// 200 symbols and counts every port and architecture, so a project this size is one a query
// cannot list in full. That is what makes it a test of the picker and the hierarchy.
for (let unit = 0; unit < 40; unit++) {
  const name = `unit${String(unit).padStart(2, "0")}`;
  const ports = Array.from({ length: 8 }, (_, port) => `    p${port} : in std_logic${port < 7 ? ";" : ""}`).join("\n");
  write(`${name}.vhd`, `library ieee;\nuse ieee.std_logic_1164.all;\n\nentity ${name} is\n  port (\n${ports}\n  );\nend entity ${name};\n\narchitecture rtl of ${name} is\nbegin\nend architecture rtl;\n`);
}

// The extension as it would ship, with the suite beside it. The suite has to live inside the
// extension to share its API object, which is what lets it answer the extension's prompts.
for (const item of ["out", "package.json", "language-configuration.json", "syntaxes", "themes"]) {
  cpSync(join(root, item), join(extension, item), { recursive: true });
}
symlinkSync(join(root, "node_modules"), join(extension, "node_modules"));
mkdirSync(join(extension, "test"));
cpSync(join(root, "test", "smoke", "suite.js"), join(extension, "test", "index.js"));

console.log(`Running in ${scratch}`);
spawnSync(
  code,
  [
    "--new-window",
    `--user-data-dir=${userData}`,
    `--extensionDevelopmentPath=${extension}`,
    `--extensionTestsPath=${join(extension, "test", "index.js")}`,
    "--disable-workspace-trust",
    workspace,
  ],
  { stdio: "inherit", env: { ...process.env, VSGRS_EXT: extension, VSGRS_RESULTS: results, VSGRS_ENTITIES: String(ENTITIES) } },
);

// The launcher returns as soon as the editor has started; the suite writes `done` when it ends.
const deadline = Date.now() + 5 * 60 * 1000;
const finished = () => existsSync(results) && /^done$/m.test(readFileSync(results, "utf8"));
while (!finished() && Date.now() < deadline) {
  await new Promise((resolve) => setTimeout(resolve, 1000));
}

const report = existsSync(results) ? readFileSync(results, "utf8") : "(the suite wrote nothing)\n";
console.log(report);
try {
  // Only the window this run started: its profile directory is unique to it.
  execFileSync("pkill", ["-f", "--", `--user-data-dir=${userData}`]);
} catch {
  // Already gone, or no pkill to ask.
}

const failed = !finished() || /^(FAIL|HARNESS ERROR)/m.test(report);
if (!failed && !process.env.KEEP) rmSync(scratch, { recursive: true, force: true });
else console.log(`Left ${scratch} for inspection.`);
process.exit(failed ? 1 : 0);

// Build one VSIX per platform, each carrying the server built for it.
//
// VS Code supports platform-specific extensions: the Marketplace serves a user the VSIX matching
// their machine, so nobody downloads six binaries to use one. The alternative -- a single VSIX
// with every binary in it -- is what an extension does when it cannot build per platform, and it
// makes the download several times larger than it needs to be.
//
//   node scripts/package.mjs --binaries <dir> [--target <vscode-target>]
//
// `<dir>` holds the release binaries, named as the release publishes them:
// `vsg-rs-<tag>-<rust-target>[.exe]`, or plain `vsg-rs`/`vsg-rs.exe` in a per-target subdirectory.

import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";

/** Every platform the release builds a server for, as VS Code names them. */
const TARGETS = {
  "linux-x64": "x86_64-unknown-linux-musl",
  "linux-arm64": "aarch64-unknown-linux-musl",
  "win32-x64": "x86_64-pc-windows-msvc",
  "win32-arm64": "aarch64-pc-windows-msvc",
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
};

function argument(name) {
  const at = process.argv.indexOf(name);
  return at === -1 ? undefined : process.argv[at + 1];
}

/**
 * The server built for `rustTarget`, wherever the caller put it.
 *
 * The release publishes archives that unpack to `vsg-rs-<tag>-<target>/vsg-rs`, so the target is
 * in a directory name rather than in the file name. Searching the tree covers that as well as a
 * plain `<target>/vsg-rs`, or a file with the target in its own name.
 */
function findServer(directory, rustTarget) {
  const wanted = rustTarget.includes("windows") ? "vsg-rs.exe" : "vsg-rs";
  const found = [];
  const walk = (at) => {
    for (const entry of readdirSync(at, { withFileTypes: true })) {
      const path = join(at, entry.name);
      if (entry.isDirectory()) {
        walk(path);
      } else if (entry.name === wanted || entry.name.includes(rustTarget)) {
        found.push(path);
      }
    }
  };
  walk(directory);
  // The target has to appear somewhere in the path, or a Linux build would answer for Windows.
  return found.find((path) => path.includes(rustTarget));
}

const binaries = resolve(argument("--binaries") ?? "binaries");
const only = argument("--target");
const root = resolve(import.meta.dirname, "..");
const serverDirectory = join(root, "server");
// The colour themes use other projects' palettes, whose licence notices have to travel with them.
const notices = ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "THIRD_PARTY_LICENSES.md"];
for (const name of notices) {
  cpSync(join(root, "..", "..", name), join(root, name));
}

let packaged = 0;
for (const [target, rustTarget] of Object.entries(TARGETS)) {
  if (only !== undefined && only !== target) {
    continue;
  }
  const server = findServer(binaries, rustTarget);
  if (server === undefined) {
    console.log(`skipping ${target}: no server for ${rustTarget} in ${binaries}`);
    continue;
  }
  // One binary at a time, so a VSIX never carries a server for another platform.
  rmSync(serverDirectory, { recursive: true, force: true });
  mkdirSync(serverDirectory, { recursive: true });
  const name = target.startsWith("win32") ? "vsg-rs.exe" : "vsg-rs";
  cpSync(server, join(serverDirectory, name));
  execFileSync(
    process.execPath,
    [join(root, "node_modules", "@vscode", "vsce", "vsce"), "package", "--target", target],
    { cwd: root, stdio: "inherit" },
  );
  packaged += 1;
}
rmSync(serverDirectory, { recursive: true, force: true });
for (const name of notices) {
  rmSync(join(root, name), { force: true });
}

if (packaged === 0) {
  console.error(`no VSIX produced: nothing matching in ${binaries}`);
  process.exit(1);
}
console.log(`packaged ${packaged} VSIX file(s)`);

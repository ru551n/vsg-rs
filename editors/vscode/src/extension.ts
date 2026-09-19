// The vsg-rs VS Code client.
//
// This extension launches `vsg-rs lsp` and speaks LSP to it. That is all it does: there is no
// VHDL parsing here, no formatter, no rules and no configuration of them. Everything a user sees
// is decided by vsg-rs itself, so an editor and the command line cannot disagree.
//
// Rules and formatting are configured in the project's own `vsg-rs.yaml`, not in VS Code
// settings. The settings here are only about which executable to run.

import { existsSync } from "node:fs";
import { chmod } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";
import { execFile } from "node:child_process";

import { ExtensionContext, OutputChannel, commands, window, workspace } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let output: OutputChannel | undefined;

/**
 * The server bundled with this extension, if this platform has one.
 *
 * A VSIX is built per platform and carries one binary, so a missing file means the user installed
 * an extension that was not built for the machine it is running on. Saying so is more use than
 * failing to spawn something that was never there.
 */
function embeddedServer(context: ExtensionContext): string | undefined {
  const name = process.platform === "win32" ? "vsg-rs.exe" : "vsg-rs";
  const path = join(context.extensionPath, "server", name);
  return existsSync(path) ? path : undefined;
}

/** The executable to run, from the settings. */
function serverPath(context: ExtensionContext): string | undefined {
  const settings = workspace.getConfiguration("vsg-rs");
  const mode = settings.get<string>("server.mode", "embedded");
  if (mode === "userPath") {
    const configured = settings.get<string>("server.path", "").trim();
    if (configured.length > 0) {
      return configured;
    }
    output?.appendLine(
      "vsg-rs.server.mode is userPath but vsg-rs.server.path is empty; using PATH instead.",
    );
    return "vsg-rs";
  }
  if (mode === "systemPath") {
    return "vsg-rs";
  }
  const embedded = embeddedServer(context);
  if (embedded === undefined) {
    output?.appendLine(
      `No server is bundled for ${process.platform}-${process.arch}. ` +
        "Install vsg-rs and set vsg-rs.server.mode to systemPath.",
    );
  }
  return embedded;
}

async function start(context: ExtensionContext): Promise<void> {
  const command = serverPath(context);
  if (command === undefined) {
    void window
      .showErrorMessage(
        `vsg-rs: no server is bundled for ${process.platform}-${process.arch}.`,
        "Show Output",
      )
      .then((choice) => {
        if (choice === "Show Output") {
          output?.show();
        }
      });
    return;
  }
  // A VSIX does not always preserve the executable bit, so the bundled server may arrive
  // unrunnable. Setting it is cheap and does nothing when it is already right.
  if (process.platform !== "win32" && command !== "vsg-rs") {
    await chmod(command, 0o755).catch(() => undefined);
  }
  const server: ServerOptions = {
    run: { command, args: ["lsp"], transport: TransportKind.stdio },
    debug: { command, args: ["lsp"], transport: TransportKind.stdio },
  };
  const options: LanguageClientOptions = {
    // Only VHDL, and only vsg-rs's own diagnostics: another server can serve the same files.
    documentSelector: [{ scheme: "file", language: "vhdl" }],
    outputChannel: output,
    // The project's own configuration file decides the rules; there is nothing to send.
    synchronize: {},
  };
  client = new LanguageClient("vsg-rs", "vsg-rs", server, options);
  try {
    await client.start();
    output?.appendLine(`Started ${command}`);
  } catch (error) {
    client = undefined;
    const message = error instanceof Error ? error.message : String(error);
    output?.appendLine(`Could not start ${command}: ${message}`);
    void window
      .showErrorMessage(
        `vsg-rs: could not start "${command}". Install vsg-rs, or set vsg-rs.server.path.`,
        "Show Output",
      )
      .then((choice) => {
        if (choice === "Show Output") {
          output?.show();
        }
      });
  }
}

/** What the extension is, and what the server it just launched reports itself to be. */
async function showVersion(context: ExtensionContext): Promise<void> {
  const extension = context.extension.packageJSON.version as string;
  const command = serverPath(context) ?? "vsg-rs";
  let server = "not found";
  try {
    const { stdout } = await promisify(execFile)(command, ["--version"]);
    server = stdout.trim();
  } catch (error) {
    server = error instanceof Error ? error.message : String(error);
  }
  output?.appendLine(`extension: ${extension}`);
  output?.appendLine(`server:    ${server}`);
  output?.appendLine(`from:      ${command}`);
  output?.show();
}

async function stop(): Promise<void> {
  const running = client;
  client = undefined;
  await running?.stop();
}

export async function activate(context: ExtensionContext): Promise<void> {
  output = window.createOutputChannel("vsg-rs");
  context.subscriptions.push(output);
  context.subscriptions.push(
    commands.registerCommand("vsg-rs.restartServer", async () => {
      await stop();
      await start(context);
    }),
    commands.registerCommand("vsg-rs.showOutput", () => output?.show()),
    commands.registerCommand("vsg-rs.showVersion", () => showVersion(context)),
    // Which executable to run is decided at startup, so a change to it needs a restart.
    workspace.onDidChangeConfiguration(async (event) => {
      if (event.affectsConfiguration("vsg-rs.server")) {
        await stop();
        await start(context);
      }
    }),
  );
  await start(context);
}

export async function deactivate(): Promise<void> {
  await stop();
}

// The vsg-rs VS Code client.
//
// This extension launches `vsg-rs lsp` and speaks LSP to it. That is all it does: there is no
// VHDL parsing here, no formatter, no rules and no configuration of them. Everything a user sees
// is decided by vsg-rs itself, so an editor and the command line cannot disagree.
//
// Rules and formatting are configured in the project's own `vsg-rs.yaml`, not in VS Code
// settings. The settings here are only about which executable to run.

import { ExtensionContext, OutputChannel, commands, window, workspace } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let output: OutputChannel | undefined;

/** The executable to run, from the settings. */
function serverPath(): string {
  const settings = workspace.getConfiguration("vsg-rs");
  const mode = settings.get<string>("server.mode", "systemPath");
  if (mode === "userPath") {
    const configured = settings.get<string>("server.path", "").trim();
    if (configured.length > 0) {
      return configured;
    }
    output?.appendLine(
      "vsg-rs.server.mode is userPath but vsg-rs.server.path is empty; falling back to PATH.",
    );
  }
  return "vsg-rs";
}

async function start(): Promise<void> {
  const command = serverPath();
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
  } catch (error) {
    client = undefined;
    const message = error instanceof Error ? error.message : String(error);
    output?.appendLine(`Could not start ${command}: ${message}`);
    void window.showErrorMessage(
      `vsg-rs: could not start "${command}". Install vsg-rs, or set vsg-rs.server.path.`,
      "Show Output",
    ).then((choice) => {
      if (choice === "Show Output") {
        output?.show();
      }
    });
  }
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
      await start();
    }),
    commands.registerCommand("vsg-rs.showOutput", () => output?.show()),
    // Which executable to run is decided at startup, so a change to it needs a restart.
    workspace.onDidChangeConfiguration(async (event) => {
      if (event.affectsConfiguration("vsg-rs.server")) {
        await stop();
        await start();
      }
    }),
  );
  await start();
}

export async function deactivate(): Promise<void> {
  await stop();
}

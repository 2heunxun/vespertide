import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  RevealOutputChannelOn,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let statusBarItem: vscode.StatusBarItem;

function getPlatformDir(): string {
  const arch = os.arch();
  const plat = os.platform();
  const key = `${plat}-${arch}`;
  const map: Record<string, string> = {
    "linux-x64": "linux-x64",
    "linux-arm64": "linux-arm64",
    "darwin-x64": "darwin-x64",
    "darwin-arm64": "darwin-arm64",
    "win32-x64": "win32-x64",
  };
  const dir = map[key];
  if (!dir) throw new Error(`Unsupported platform: ${key}`);
  return dir;
}

function resolveServerBinary(context: vscode.ExtensionContext): string {
  const config = vscode.workspace.getConfiguration("vespertide");
  const override = config.get<string>("serverPath");
  if (override && override.trim() !== "") {
    if (!fs.existsSync(override)) {
      throw new Error(`vespertide.serverPath points to a non-existent file: ${override}`);
    }
    return override;
  }
  const exe = os.platform() === "win32" ? "vespertide-lsp.exe" : "vespertide-lsp";
  const bundled = context.asAbsolutePath(path.join("bin", getPlatformDir(), exe));
  if (!fs.existsSync(bundled)) {
    throw new Error(
      `Bundled Vespertide LSP binary not found at: ${bundled}\n` +
        `Set "vespertide.serverPath" to the binary location, or reinstall the extension.`
    );
  }
  return bundled;
}

function createStatusBarItem(): vscode.StatusBarItem {
  const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  item.text = "$(loading~spin) Vespertide";
  item.tooltip = "Vespertide Language Server";
  item.command = "vespertide.restartServer";
  item.show();
  return item;
}

async function startClient(context: vscode.ExtensionContext): Promise<void> {
  let serverPath: string;
  try {
    serverPath = resolveServerBinary(context);
  } catch (err) {
    statusBarItem.text = "$(error) Vespertide: Not Found";
    void vscode.window.showErrorMessage(`Vespertide: ${(err as Error).message}`);
    return;
  }

  const config = vscode.workspace.getConfiguration("vespertide");
  const logLevel = config.get<string>("logLevel", "info");

  const serverOptions: ServerOptions = {
    run: {
      command: serverPath,
      args: [],
      transport: TransportKind.stdio,
      options: { env: { ...process.env, RUST_LOG: `vespertide_lsp=${logLevel}` } },
    },
    debug: {
      command: serverPath,
      args: [],
      transport: TransportKind.stdio,
      options: { env: { ...process.env, RUST_LOG: "vespertide_lsp=trace" } },
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "vespertide-json" },
      { scheme: "file", language: "vespertide-yaml" },
    ],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher(
        "**/{models,migrations}/*.{json,yaml,yml}"
      ),
    },
    revealOutputChannelOn: RevealOutputChannelOn.Error,
    traceOutputChannel: vscode.window.createOutputChannel("Vespertide LSP Trace"),
  };

  client = new LanguageClient("vespertide", "Vespertide", serverOptions, clientOptions);

  try {
    await client.start();
    statusBarItem.text = "$(check) Vespertide";
    statusBarItem.tooltip = "Vespertide Language Server (connected)";
  } catch (err) {
    statusBarItem.text = "$(error) Vespertide";
    void vscode.window.showErrorMessage(`Vespertide LSP failed to start: ${err}`);
  }
}

async function stopClient(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  statusBarItem = createStatusBarItem();
  context.subscriptions.push(statusBarItem);

  context.subscriptions.push(
    vscode.commands.registerCommand("vespertide.restartServer", async () => {
      statusBarItem.text = "$(loading~spin) Vespertide: Restarting";
      await stopClient();
      await startClient(context);
    })
  );

  await startClient(context);
}

export async function deactivate(): Promise<void> {
  await stopClient();
}

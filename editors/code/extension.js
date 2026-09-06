// The extension is a thin spawn of `balaur lsp`: everything it reports comes
// from the engine, so a popup here says what the Script persona says.

const fs = require("fs");
const path = require("path");
const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

// The server boots a project, so it needs the directory holding project.toml,
// not the file being edited. The setting wins where a workspace holds several.
function projectRoot(folder) {
  const configured = vscode.workspace.getConfiguration("balaur").get("projectRoot");
  if (configured) {
    return path.resolve(folder.uri.fsPath, configured);
  }
  const root = folder.uri.fsPath;
  if (fs.existsSync(path.join(root, "project.toml"))) {
    return root;
  }
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    if (entry.isDirectory() && fs.existsSync(path.join(root, entry.name, "project.toml"))) {
      return path.join(root, entry.name);
    }
  }
  return root;
}

async function start() {
  const folder = vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders[0];
  if (!folder) {
    return;
  }
  const command = vscode.workspace.getConfiguration("balaur").get("serverPath") || "balaur";
  const root = projectRoot(folder);
  const server = {
    command,
    args: ["lsp", root],
    transport: TransportKind.stdio,
    options: { cwd: root },
  };
  client = new LanguageClient("balaur", "Balaur", { run: server, debug: server }, {
    documentSelector: [{ scheme: "file", language: "rune" }],
    outputChannelName: "Balaur",
  });
  await client.start();
}

async function stop() {
  if (client) {
    await client.stop();
    client = undefined;
  }
}

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("balaur.restartServer", async () => {
      await stop();
      await start();
    }),
  );
  await start();
}

module.exports = { activate, deactivate: stop };

# Balaur for VS Code

Rune script support for a Balaur project, from `balaur lsp`. What the
extension shows is what the engine knows, so a completion here and a
completion in the Script persona come from the same place.

- Diagnostics, on every keystroke, from the buffer rather than the file.
- Completion for engine modules, node methods, component handles, constants,
  the script's own functions and Rune's own methods.
- Hover docs and signature help, the same lines as the reference.
- Go-to-definition, document and workspace symbols, and find references.
- Formatting, with Rune's own formatter.
- Rename and find-and-replace across the files a script's `mod` declarations
  reach.

## Install

The extension spawns `balaur`, so install the engine first and put it on
`PATH`, or set `balaur.serverPath` to its absolute path.

```bash
cd editors/code && npm install && npx @vscode/vsce package
```

Then install the `.vsix` from the Extensions view's `…` menu.

## Settings

- `balaur.serverPath` — the binary to run. Default `balaur`.
- `balaur.projectRoot` — the project to boot, relative to the workspace
  folder. Default: the folder holding `project.toml`.

## Other editors

The server is `balaur lsp <project>` over stdin and stdout, so any LSP client
runs it.

**Neovim**, with `nvim-lspconfig` loaded:

```lua
vim.lsp.config.balaur = {
  cmd = { "balaur", "lsp", "." },
  filetypes = { "rune" },
  root_markers = { "project.toml" },
}
vim.lsp.enable("balaur")
```

**Zed**, in a language server extension or `settings.json`:

```json
{ "lsp": { "balaur": { "binary": { "path": "balaur", "arguments": ["lsp", "."] } } } }
```

## Third party

`syntaxes/balaur.tmLanguage.json` and `language-configuration.json` are the
Rune project's, MIT OR Apache-2.0, themselves derived from rust-analyzer's.

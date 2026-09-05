> **Status:** not started. Written down on 2026-09-05. Spline V2 opened its
> editor to outside agents through an MCP server and built an agent of its
> own on the same tools; both are days of work here rather than a product,
> because a Balaur project is text on disk, the editor is scripts that
> reload when the disk changes, and the engine already describes its own
> API. The order is files first, because an agent that edits the project
> through the same seam a person does needs no editor running; then the
> live editor, for the things only it knows; then a remote transport.

# Plan: an MCP server

`balaur mcp`: the Model Context Protocol over stdio, exposing the project,
the engine's API and the running editor as tools an agent — Claude Code,
Cursor, an IDE, or an agent shipped in the editor later — drives.
Everything it can do, `balaur` and the editor can already do; this is one
protocol over those verbs.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| The script API, components, assets and constants as JSON | `balaur api`, `docs/generated/api.json` |
| A project checked without running it, every finding with a line | `balaur check --strict` |
| A language server and a debug adapter over a port | `balaur lsp`, `balaur run --debug` |
| Tests, import, export, a headless run with a digest and a log | `balaur test`, `balaur import`, `balaur export`, `balaur run --headless`, `logbuf` |
| Offscreen screenshots | `balaur run --offscreen`, `render.screenshot` |
| Scene edits that keep comments | the TOML patcher |
| Scripts and assets reloaded when the file changes, in a game and in the editor | `App::watch`, asset generations |
| Every editor command as a script, and named states for its tests | `palette.rn`, `selftest.rn`, `--state` |
| Component schemas, presets and tags a caller can enumerate | `scene.component_schema`, `presets`, `component_tags` |
| `llms.txt` on the site | `../balaur-website/static/llms.txt` |

Missing:

- **An MCP endpoint.** Nothing speaks the protocol.
- **A way into the running editor.** The debug adapter reaches a game, not
  the editor's selection, viewport or commands.
- **Tool schemas.** The verbs exist; their input and output shapes are argv
  and stdout.

## 1. Design

**Files are the API.** The project is TOML and Rune on disk and the editor
reloads what changes, so the server edits files and the editor follows.
`balaur mcp` runs a headless engine over the project, with `rmcp` — the Rust
SDK maintained under the protocol's own organisation — over stdio, the
transport every client speaks. Tools:

| Tool | Does |
| --- | --- |
| `project.info`, `scene.list`, `scene.read`, `asset.list`, `asset.read`, `script.read` | Read |
| `node.add`, `node.patch`, `node.remove`, `node.move`, `asset.write`, `script.write` | Write, through the TOML patcher, under the project root only |
| `component.types`, `component.schema`, `component.presets`, `api.search` | The engine describing itself, from `api.json` |
| `check` | `balaur check --strict`, findings as structured output |
| `run` | N ticks headless, the digest and the log |
| `screenshot` | An offscreen frame at a camera, as an image |
| `test`, `export` | The commands |

Resources: `balaur://api`, `balaur://components`, `balaur://scene/<path>`,
`balaur://manual/<page>`. Prompts: "add a component", "make a scene",
"explain this hook".

**The live editor is a second transport, and the tools are the palette.**
`balaur edit --mcp <port>` opens a JSON-RPC listener beside the debug
adapter's, and the editor answers `on_agent_request` in Rune:
`editor.selection`, `editor.select`, `editor.command(name)` for any palette
entry, `editor.state(name)` for any self-test state, `editor.viewport` for a
capture of what the person sees, `editor.play`, `editor.stop`. `balaur mcp
--attach <port>` proxies those as tools, so one server offers both tiers. No
second vocabulary: a command an agent can run is one a person can, by name.

**Writes are the developer's.** The server runs on the developer's machine
with their permissions and writes only under the project; `engine.open_url`,
an export to a store and anything with a credential are never tools. Rows an
agent reads from a project are data, not instructions.

**An agent in the editor is the same client.** A chat dock that talks to a
model and drives these tools is a product decision, not engine work; if it
is built, it is built as an MCP client so it and Cursor are the same thing.

**Generation is an extension.** Text-to-3D and image generation are vendor
APIs; a plugin that calls one and writes a `.glb` or a `.png` into the
project is the whole integration, and drag-in
(`docs/PLAN-editor-ergonomics.md`) does the rest. Not engine work.

**The browser editor gets it over Gamend.** No stdio in a tab; the same tool
set over a Gamend socket is `docs/PLAN-collaboration.md`'s, once a project
lives there.

## 2. The surface

| Piece | Decision |
| --- | --- |
| `balaur mcp` over stdio, `rmcp` | Step 1 |
| File tools, describe tools, `check`, `run`, `screenshot` | Step 1 |
| Resources and prompts | Step 1 |
| `--mcp` on the editor, `--attach` on the server | Step 2 |
| Streamable HTTP transport | Step 3, for a remote client, behind the same tools |
| An agent dock in the editor | Not planned here; an MCP client if ever |
| Generation | Not planned; an extension |
| Sampling (the server asking the client's model) | Not planned |

## 3. Steps

1. **Files.** The server, the tools, resources, prompts, a manual page on
   connecting Claude Code and Cursor. Ends with: an agent adding a lit cube
   to `examples/hello` and the editor showing it without a restart.
2. **Live.** The editor listener, `on_agent_request`, the proxy. Ends with:
   an agent selecting a node and taking a capture of the viewport.
3. **Remote.** The HTTP transport.

## 4. What CI can prove, and what it cannot

- Every tool has a test that calls it over stdio against an example project
  and checks the file it wrote, headless.
- The tool list and its schemas are generated into `docs/generated/` and
  checked for drift like `api.json`.
- The editor's `on_agent_request` runs in `selftest.rn`.
- What it cannot: whether an agent uses the tools well. That is an eval, not
  a test, and belongs beside the site's `llms.txt`.

## 5. Open questions

1. **`rmcp`'s pace.** The SDK moves with the protocol; pinning it is a
   `deny.toml` question like any other dependency.
2. **Concurrent writers.** An agent and a person editing one scene is the
   same race two people are; the lock in `docs/PLAN-collaboration.md`
   answers both.
3. **What `screenshot` costs.** An offscreen engine per call is seconds;
   keeping one warm is a process the server owns.

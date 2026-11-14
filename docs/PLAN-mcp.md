# Plan: an MCP server

`balaur mcp`: the Model Context Protocol over stdio, exposing the project,
the engine's API and the running editor as tools an agent drives. Every tool
is a verb `balaur` or the editor already has, so this is one protocol over
those verbs.

## Steps

1. **Files.** `balaur mcp` runs a headless engine over the project with
   `rmcp` over stdio. The project is TOML and Balaur Script on disk and the
   editor reloads what changes, so the server edits files and the editor
   follows. The tools:
   - Read: `project.info`, `scene.list`, `scene.read`, `asset.list`,
     `asset.read`, `script.read`.
   - Write, through the TOML patcher and under the project root only:
     `node.add`, `node.patch`, `node.remove`, `node.move`, `asset.write`,
     `script.write`.
   - Describe, from `api.json`: `component.types`, `component.schema`,
     `component.presets`, `api.search`.
   - Run: `check` (`balaur check --strict`, findings as structured output),
     `run` (N headless ticks, the digest and the log), `screenshot` (an
     offscreen frame), `test`, `export`.

   Resources `balaur://api`, `balaur://components`, `balaur://scene/<path>`
   and `balaur://manual/<page>`, a few prompts, and a manual page on
   connecting a client. The tool list and schemas are generated into
   `docs/generated/` and checked for drift. Ends with: an agent adds a lit
   cube to `examples/hello` and the editor shows it without a restart.
2. **Live.** `balaur edit --mcp <port>` opens a JSON-RPC listener, and the
   editor's scripts answer `on_agent_request`: `editor.selection`,
   `editor.select`, `editor.command(name)` for any palette entry,
   `editor.state(name)` for any self-test state, `editor.viewport`,
   `editor.play` and `editor.stop`. `balaur mcp --attach <port>` proxies them,
   so one server offers both tiers. Ends with: an agent selects a node and
   takes a capture of the viewport, and `selftest.bs` runs
   `on_agent_request`.
3. **Remote.** The streamable HTTP transport, behind the same tools.

Never tools: `engine.open_url`, an export to a store, and anything holding a
credential. Not planned: an agent dock in the editor (if built, an MCP
client), generation (an extension that writes files into the project), and
sampling.

## Open questions

1. **`rmcp`'s pace.** The SDK moves with the protocol; pinning it is a
   `deny.toml` question.
2. **Concurrent writers.** An agent and a person editing one scene race as two
   people do; the lock in `docs/PLAN-collaboration.md` answers both.
3. **What `screenshot` costs.** An offscreen engine per call takes seconds;
   keeping one warm is a process the server owns.

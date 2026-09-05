> **Status:** not started. Written down on 2026-09-05. Spline's files live
> in the cloud, every prompt is a version, several people edit one scene at
> once and leave comments on it. `docs/PLAN-web-editor.md` said two people
> editing one project is a product, not a port, and left it out; this is
> that product written down so it can be ranked. The order is the cheap
> half first — a project on an account, a share link, versions, presence,
> comments — because each stands on Gamend as it is; simultaneous editing
> last, because it is the only part that needs a new data structure, and
> because a lock gets most of the value first.

# Plan: projects in the cloud

Projects on a Gamend account, share links with roles, a version per save,
presence in the viewport, comments anchored to nodes, and editing together —
first under a lock per scene, then over a CRDT if a team asks.
`docs/PLAN-web-editor.md` step 5 is the first step here, seen whole.

## 0. Where the tree is today

Built, and not built for this:

| Have | Where |
| --- | --- |
| Accounts, REST, realtime channels and hooks, on native and in the browser | `balaur_gamend`, the Phoenix client, `crates/balaur_gamend/src/browser.rs` |
| The browser editor with the project mirrored in the tab and restored on refresh | `crates/balaur_cli/src/web_fs.rs::StorageFs` |
| Undo with labels, and a dirty flag per file | `editor/scripts/history.rn` |
| Selection highlights and gizmos drawn over the mirror | `highlight.rn`, `gizmo.rn` |
| Scene edits that keep comments | the TOML patcher |
| A file backend seam, one implementation per platform | `balaur::files` |
| Object storage, an admin portal, and the split between server work in the `gamend` repository and engine work here | `docs/PLAN-gamend.md` §2 |

Missing:

- **A project on the server.** Gamend has no projects table; the browser
  editor's files live in `localStorage`.
- **A link.** Nothing names a project outside the machine it is on.
- **Versions.** Save overwrites.
- **Presence and comments.** No channel carries who is here or what they
  said.
- **Two writers.** Two tabs on one project overwrite each other.

## 1. Design

**A project is files under an id, and every save is a version.** Gamend
grows three tables — server work in the `gamend` repository, kept in step
here as `docs/PLAN-gamend.md` does: `projects` (owner, name, visibility),
`files` (project, path, blob hash, version) and `versions` (project, label,
author, parent, time). Blobs go to the object storage Gamend already has. ⌘S posts the changed files with the `history` label as the version's,
so the history a person sees in the editor and the history on the server are
one list. A version restores as a whole; a file's history is a diff view in
the editor. Blobs are content-addressed, so a texture saved twice is stored
once.

**A `FileBackend` over Gamend.** The fourth backend after `std::fs`,
`StorageFs` and the memory one: reads through a local cache, writes queue and
post, and `localStorage` stays the offline mirror so a tab keeps working on a
train. Native editors use it too, so a desktop and a browser open the same
project.

**A share link opens the editor.** `balaurengine.org/editor?project=<id>`
with a role from the link — viewer or editor — and the play URL from
`docs/PLAN-deploy.md` beside it. Roles are Gamend's; the editor only asks.

**Presence is a channel.** `project:<id>` carries who is in it, their
selection and their camera, a few times a second; the editor draws each
person's selection as an outline in their colour and their camera as a small
frustum, through the highlight code that already draws the local one. The
channel also carries file-changed events, so a save in one tab reloads in
another — what the watcher does on a disk.

**Comments are data on the server, never in the project.** A comment is
`(project, scene, node path, point, text, author, resolved)` in a `comments`
table and channel, drawn as pins in the viewport and listed in a Comments
dock, with mentions and resolve. A renamed node keeps its comments through
the stable ids `docs/PLAN-scenes-and-assets.md` phase 3 adds; until then, a
path.

**Editing together, in two stages.** First, a lock per scene: the first
editor to write holds it, others see live changes and may request it, and a
lock expires with its holder's presence. It needs only the channel and the
file table and gives most of what a small team wants. Second, if a team
asks, a CRDT over the node table: `automerge` — a JSON-shaped document with a
Rust core — holding each scene as nodes of properties, with the TOML file as
the materialised view written on save so git and the patcher keep working;
scripts as text CRDTs from the same library. `yrs` is the alternative with
the same shape. The editor's `S.doc` already is a node table, which is what
makes the mapping honest. Operational transform is not planned.

**The catalogue.** Once files live on Gamend,
`docs/PLAN-editor-ergonomics.md`'s library grows a hosted half: a
`catalogue` table with the same manifest as `editor/library/`, publishable
from a project. The MCP tools over a Gamend socket (`docs/PLAN-mcp.md`)
arrive with the same channel.

## 2. The surface

| Piece | Decision |
| --- | --- |
| Projects, files, versions on Gamend; the `FileBackend` | Step 1 |
| Share links and roles | Step 1 |
| Version list, restore, per-file diff | Step 2 |
| Presence: selection, camera, file-changed | Step 3 |
| Comments, pins, dock, mentions | Step 4 |
| A lock per scene | Step 5 |
| A CRDT, `automerge` (or `yrs`) | Step 6, on demand |
| Operational transform | Not planned |
| A hosted catalogue | Step 7 |
| MCP over Gamend | `docs/PLAN-mcp.md` step 3 over this plan's channel |
| Voice or video in the editor | Not planned |
| Self-hosting Gamend for a studio | Have: it is a server a studio runs; nothing here assumes `balaurengine.org` |

## 3. Steps

1. **A project on the server.** The tables, the backend, sign-in from the
   editor, open and save, the link. Ends with: the same project opened from
   two machines.
2. **Versions.**
3. **Presence.**
4. **Comments.**
5. **The lock.**
6. **The CRDT**, if asked.
7. **The catalogue.**

## 4. What CI can prove, and what it cannot

- The `FileBackend` runs the suite `std::fs` and `StorageFs` run, against a
  Gamend started in the job.
- A save from one client reaching another is a two-client test over the
  channel, headless.
- A lock handoff and its expiry are the same test with a dropped client.
- What it cannot: how presence feels at real latency, and whether a CRDT
  merge ever produces a scene nobody meant. The second is why the lock comes
  first.

## 5. Open questions

1. **Who pays for storage.** A free tier with a quota, or bring-your-own
   Gamend. Both are the same code; the number is a product decision.
2. **Binary assets in a CRDT.** They are not; a texture is a blob with a
   version, and only text goes through `automerge`.
3. **The browser's `FileBackend` is synchronous.** `StorageFs` exists
   because OPFS is not; a Gamend backend queues writes, which means a save
   can fail after ⌘S returned. The editor needs a "saving…" state it does
   not have.

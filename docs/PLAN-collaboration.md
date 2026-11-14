# Plan: projects in the cloud

Projects on a Gamend account: share links with roles, a version per save,
presence in the viewport and comments anchored to nodes. Editing together
comes under a lock per scene, then over a CRDT if a team asks. Server tables
are work in the `gamend` repository, kept in step here as
`docs/PLAN-gamend.md` does.

## Steps

1. **A project on the server.** Gamend grows `projects` (owner, name,
   visibility), `files` (project, path, blob hash, version) and `versions`
   (project, label, author, parent, time), with blobs content-addressed in
   its object storage. A `FileBackend` over Gamend reads through a local
   cache and queues writes, with the IndexedDB mirror
   (`crates/balaur_cli/src/web_store.rs`) kept for offline use; native editors
   use it too. A share link, `balaurengine.org/editor?project=<id>`, carries a
   viewer or editor role that Gamend holds. Ends with: the same project
   opened from two machines, and the backend passing the suite `DiskFs` and
   `MemoryFs` pass against a Gamend started in the job.
2. **Versions.** ⌘S posts the changed files under the `history` label, so the
   editor's undo history and the server's are one list. A version restores
   whole, and a file's history is a diff view.
3. **Presence.** A `project:<id>` channel carries who is in the project, their
   selection and their camera a few times a second, and file-changed events
   so a save in one tab reloads in another. The editor draws each person's
   selection in their colour and their camera as a small frustum. Ends with: a
   two-client headless test sees a save arrive.
4. **Comments.** `(project, scene, node, point, text, author, resolved)` in a
   `comments` table and channel, keyed by `StableId`, drawn as pins and
   listed in a Comments dock with mentions and resolve. Comments never enter
   the project.
5. **The lock.** The first editor to write a scene holds it, others see live
   changes and may request it, and it expires with its holder's presence.
   Ends with: a handoff and an expiry tested with a dropped client.
6. **The CRDT, if asked.** `automerge` (or `yrs`) over each scene's node
   table, fed by the live scene's change log (`scene.keep_changes`), with the
   TOML file written on save so git and the patcher keep working. Scripts are
   text CRDTs from the same library, and binary assets stay versioned blobs.
7. **The catalogue.** A Gamend `catalogue` table with the manifest
   `editor/library/catalog.toml` already uses, publishable from a project.

Not planned: operational transform, and voice or video in the editor.

## Open questions

1. **Who pays for storage.** A free tier with a quota, or bring-your-own
   Gamend. Both are the same code; the number is a product decision.
2. **A save that fails late.** A Gamend backend queues writes, so a save can
   fail after ⌘S returned. The editor needs a "saving…" state it does not
   have.

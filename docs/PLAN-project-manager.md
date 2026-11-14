# Plan: project manager

What is left of the screen the editor starts on when it was given no project
(`editor/scripts/manager.bs`, over `project::*` and `release::*`).

## Steps

1. **A new project in a browser tab.** The tab lists and opens projects, but
   `project::create` has no web half: `crates/balaur_cli/src/project_web.rs`
   answers `recent`, `open` and `forget` only, and `new_project.rs` writes
   through `std::fs`. Copy a template through the files backend, and fetch an
   example's export pack from the page. Done when New and an example card work
   in a tab and the page drops its own two start buttons.
2. **A dirty document asks first.** `manager::go` calls `project::open`, which
   quits this process, without looking at `history::dirty`. Ask to save, or
   save, before it does. Done when opening a project from a scene with
   unsaved edits keeps them.

## Open questions

- Spawning from a macOS bundle: should the parent quit only once the child has
  started? On Windows, does a GUI process spawning the console binary flash a
  console?

# The engine holds core features only

What a script can build from the engine's functions moves to scripts, and the
engine keeps a primitive only where a script cannot reach. `AGENTS.md` states
the rule; `ARCHITECTURE.md` "What stays native" holds what stays and why.

## Left

1. `tilemap.terrain` is both a property and a method. The method that reads
   one cell's terrain takes a name of its own.
2. `panel` goes once `column` draws a caption and a nine-patch plate, which
   `dialog` shares.
3. Camera follow moves to the library rigs once a script can set a node's
   world-space position; the modifier writes the follower in its moving
   parent's space.
4. The `render::text_size` test needs `balaur_render`'s `window` feature,
   which no CI test job turns on.

## Not planned here

- Tweens stay native: `then`, `on_tween_finished(handle)` and the step events
  need a script that owns them, which a generated clip on a child node is not.
- ⌘Q on macOS leaves through AppKit's `terminate:`, which closes the windows
  without a close request, so `on_quit_requested` does not run. Routing it
  needs AppKit bindings beside winit's own; winit offers only turning its menu
  off, which takes Hide and Quit with it.

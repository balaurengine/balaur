> **Status:** every step is built. The kiss3d half waits for its commit, and
> Balaur builds against `../kiss3d` until the pin moves. Written 2026-11-11
> from issue 78 and an audit of kiss3d's window, the render loop and input,
> done by reading. A step marked run was reproduced on a Mac.

# Plan: what the engine holds, and when it goes stale

Issue 78: on a 14-inch MacBook every click in the editor landed away from the
pointer, further the lower and the more to the right. The editor asked for a
1600 by 1000 window, AppKit shrank it to fit the screen, and kiss3d kept its
surface at the size asked for. The frame was squeezed into the window and the
pointer was not.

That is one case of a wider pattern: a value read once, or carried by an
event, trusted after the thing it describes has moved. This plan lists every
such value the audit found, in the order they get fixed.

## 0. Rules

- A held value names its source: a size and a scale come from the window, a
  held key from the key events and the focus. The code that keeps a copy says
  when it reads the source again.
- An event's payload can be stale. Read the source when the event arrives,
  and use the event only as the signal that something moved.
- A window call made between pumps can lose its event. winit on macOS drops an
  event raised with no handler set, and kiss3d calls `set_visible` and
  `set_maximized` outside a pump, so what such a call changes is checked on the
  next poll.
- One fact lives in one place. Two copies of a scale or a screen size disagree
  sooner or later; the second one is a read of the first.
- Re-reading a source is not the same as asking it every time. A value worked
  out from settings is read through `settings::kept`, which keeps it until a
  setting or this run's tags move (`UiSettings::of`, `WindowSettings::of`);
  a fact about the device is read from `DeviceFacts`, which the frame probe in
  `device.rs` refreshes, asking the system once a frame or less. Nothing reads
  a file each frame.

## 1. The surface follows the window (run, built in the fork)

kiss3d opens the window hidden, pumps once to attach the screen reader, then
shows it. During that pump winit reported the hidden window's size, 1600 by
1000. AppKit shrank the window when it was shown, and the next drain applied
the stored size to the surface. Maximizing after the window opened lost its
`Resized` the same way.

`PendingEvent::Resize` now carries no size: the drain reads the window. On
macOS and the web every poll compares the surface with the window, and
`drawable_size` answers per platform (`outer_size` on iOS, where
`inner_size` is the safe area). Android's resume rebuilds the depth and MSAA
targets with the surface. `tests/surface_follows_window.rs` opens both
windows on a real display; before the fix it found a 12000 by 12000 surface
in a 2880 by 1476 window, and 640 by 640 in a maximized one.

Left: commit in `../kiss3d`, then move the two kiss3d lines in `Cargo.lock`.

## 2. The window opens as its own project says (run, built)

The editor's `init` merges the game's `project.toml` into the settings it
reads itself (`editor/scripts/settings.bs`, `load_files`). That runs inside
`load_project`, before the window opens, so `balaur edit` on a game with a
1920 by 1080 `[window]` opened the editor at 1920 by 1080.

`WindowSettings::opening` reads `[window]` from the running project's own
manifest on the tags this run answers to, and everything that opens a window
reads it. The editor's `project.toml` states its own size, 1280 by 800, the
size its shell seeds before the window reports one.

## 3. The asked size fits the monitor (built)

Nothing compared `[window]` with the screen. macOS shrinks the window;
Windows and Wayland leave it hanging off the screen. kiss3d now asks the
monitor before it creates the window, and one that does not fit opens
maximized: winit has no work area, so the system sizes it to the screen less
its taskbar or Dock. Restored, it is the most the monitor holds. Wayland names
no primary monitor, so there the smallest one decides.

## 4. Losing focus lets go of held keys and buttons (built)

`key_states` and `button_states` in `wgpu_canvas.rs` change only on key and
button events, and winit sends no releases on macOS, Wayland or the web.
Hold W, switch apps, let go, and the character keeps walking; ⌘ held through
⌘-Tab turns a later S into ⌘S. Balaur's input maps `Focus` to a flag only
(`kiss3d_input.rs`), and egui never hears `WindowFocused(false)`.

kiss3d keeps what is held as two sets instead of two arrays. On
`Focused(false)`, and on the page's `blur` on the web, it sends a release for
everything still held, and egui gets `WindowFocused(false)`.

## 5. The picking ray uses this frame's pointer (run, built)

`publish_camera` runs before `pump_input` in `kiss3d_backend.rs` `step`, so
`render::mouse_ray` is last frame's. A fast click picks under the old
position, and a tap's press and release can land on different nodes, which
turns the click into a drop. The camera is published after input now. No
suite runs the windowed step, so an offscreen probe checked it: before, the
ray moved a frame after `input::mouse_position()`.

## 6. egui's own keyboard zoom is off (built)

egui zooms on ⌘+, ⌘− and ⌘0 by itself (`Options::zoom_with_keyboard`).
Since kiss3d `603ea30a` it sees ⌘, so it fights the editor's own zoom for a
frame, and under `low_processor` that frame can stay on screen. kiss3d turns
the option off; Balaur owns the zoom.

## 7. The pointer leaving the window ends hover (built)

kiss3d dropped `CursorLeft`, so egui never got `PointerGone`: hover and
tooltips stayed up and the UI kept the pointer. kiss3d now sends
`CursorEnter`, clears its cursor position on the way out, and egui gets
`PointerGone`; the web sends the same from `pointerleave`.

`InputSnapshot` records that the pointer is outside, defaulting to inside in an
older session, and `interact` treats an outside pointer as over nothing, so
the node it was over hears `pointer_exit`.

## 8. The first mouse move is not a delta (built)

Balaur's mouse delta started from (0, 0), so the first move reported the whole
position and mouse-look snapped, and coming back in at another edge reported
the jump. The first position after start, or after the pointer left, places it
and moves nothing.

## 9. Web input in canvas pixels (built, not run in a browser)

Touch events passed CSS `client_x` with no canvas offset and no device pixel
ratio, while the mouse applied both. On a phone at a ratio of 3 a tap landed
at a third of its position. Touch now goes through the mouse's conversion. A
press captures the pointer, so a release off the canvas arrives, and `blur`
lets go of what is held.

## 10. The editor's process settings are its own (built)

The editor folded the game's manifest into the settings it runs with, and its
Settings screen wrote from that same store. Saving the game's settings wrote
the editor's own `low_processor`, `strict` and `[ui]` lines into the game's
`project.toml` wherever the game left them out, and the game's `msaa` and
`vsync` drove the editor's window.

The editor's `settings.bs` holds the game's manifest apart from this run and
writes it back with `toml::patch`, which changes only the keys that moved; a
script's own `set` changes this run only, so a game played in the editor
never writes its file. The editor runs with every value in it but a setting
marked `host = true`: the window, `[check]`, the application's own files and
the `[ui]` class lines.
Play in the editor still runs on the game's physics and time. Every setting is
listed in `docs/generated/settings.md`, to check before declaring another.

## 11. A script's `init` sees the device (built)

`init` runs inside `load_project`, before the window exists. `[ui]` is read on
the first ask now rather than on the first tick, so `init` sees the project's
scale and class lines, and before the window opens `ui::window_size()` answers
`[window]` in design pixels at that scale. Every entry point seeds dark mode
before the project loads. `on_resize` tells the first real size too, which
corrects a layout `init` made against the guess. The headless editor publishes
the screen its pass draws on, so a tick and a pass agree.

## 12. One UI scale (built)

`ui::pixels_per_design()` answers the engine's own window pixels per design
pixel, the system text size included, and the editor's conversions read it
instead of their own copy. Stopping play puts the editor's scale back after a
game's `ui::set_scale`. The `ui_scale` setting applies at once, as it says now;
⌘= and ⌘− take one path that writes and saves it, keeping a touch screen's
floor. `ui::set_scale` asks for a pass.

## 13. The 2D camera follows a change of display (built)

Scripts are the only thing that moves the 2D zoom, so a zoom that no longer
matches the asked one at the window's scale is re-applied: a window dragged to
a display with another scale draws the 2D world at its size.

## 14. `refresh_rate` is the display's (built)

kiss3d answers `Window::refresh_rate` from the monitor the window is on. Where
the system gives none, the measured median counts only frames short enough to
be a display's, not a lazy loop's sleep.

## 15. Phones and tablets (built, not run on a device)

- Android keeps the navigation bar's share per window height, and a height
  first seen with the keyboard asked for borrows the last one's.
- A tap while a field keeps focus shows the keyboard again.
- Android reports its safe area from the content rect, less the keyboard.
- kiss3d acquires no frames between `Suspended` and `Resumed`.

## 16. Text input and the cursor (built)

- egui's IME caret goes to `set_ime_cursor_area`, so the candidate window opens
  beside it.
- A grab keeps the cursor inside the window where the system allows that,
  else locks it in place (macOS, the web). While locked, raw motion arrives as
  `WindowEvent::MouseMotion` and adds to `input::mouse_delta()`.

## 17. Read once, but can change (built)

- On Linux and Windows a dark mode change wakes a `low_processor` loop;
  macOS's own event already did.
- `touchscreen` asks the system: Windows counts touch points, Linux reads
  udev's device database, a Mac has none.

## 18. A lost surface reads the window again (built)

On `Outdated` or `Lost`, `get_current_texture` takes the window's size before
it reconfigures; the method takes `&mut self` for that.

## 19. The editor remembers its window (built)

The editor's scripts keep its size in `window.toml` in the editor's data folder
when the window closes, or before the manager opens another project. `init` sets
`window/width` and `window/height` from it, and that runs before the window
opens. A run started with `--state`, a page and a phone keep nothing. The engine
has no setting for it: a game that wants the same does it in its own scripts.

# Plan: the editor on a tablet and a phone

What is left before the editor's shell is used under a finger on a tablet and
a phone. The screen and input classes, their override tables and the shell's
folding are built, and every chord has a palette line; the shots are taken by `scripts/uiaudit.sh` at each class.

## Steps

1. **The editor project in the iOS bundle.** `scripts/package_runtime.sh
   ios-sim` writes an unsigned bundle of the CLI alone, so it has no editor to
   start. Carry the editor project in it, as `scripts/package.sh` does for the
   desktop. Done when the simulator opens the project manager.
2. **A tablet and a phone in the simulator.** Open, edit and play a project at
   a tablet's size and at a phone's in both orientations, and read the class
   each reports. Done when each passes and the shots join
   [EDITOR-SCREENS.md](EDITOR-SCREENS.md).
3. **One layout pass for the docks and the chrome.** `layout::stage_room` in
   `editor/scripts/layout.bs` works out the room the docks leave before the
   layout runs, repeating its widths. Lay out the docks first, then the chrome
   against what they left. Done when no `stage_room` is left and the layout
   assertions hold at every audited size.

## Open questions

- Does a phone want a bottom bar on `narrow`, beyond the folded docks? None is
  designed yet.
- Does `medium` earn its keep? Portrait tablets and large phones held sideways
  land in it, and only the editor's half-screen dock answers to it.
- Does a `large` width word belong near 1200? Android added one in 2024, and
  the editor's `COMPACT_WIDTH` sits at 1240.
- Should a widget's `kind` turn by class? A row that becomes a column on
  `narrow` takes a `flow` or two nodes today.
- Android's text size waits on the GameActivity glue reaching
  `Configuration.fontScale`.

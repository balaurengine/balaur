# Plan: the editor shell's look

What is left of the shell's craft pass, against the screen catalogue in
[EDITOR-SCREENS.md](EDITOR-SCREENS.md). `D1`…`D17` are that file's defect
numbers.

## Steps

1. **Keys a drag moves (D11).** `timeline_fill` in `editor/scripts/dock.bs`
   places each key as a `●`/`○` text button that only seeks. Draw a key as a
   circle with a hit target and let a drag along its lane move its time. Done
   when a dragged key's new time is in the clip, one undo puts it back, and a
   self-test drags one.
2. **Asset tiles that show the asset (D12).** The cards in
   `editor/scenes/panels/assets.toml` carry an icon and a name. Give an image
   file's card its thumbnail and each card a colour for its type. Done when a
   texture's card shows its picture.
3. **Golden screens.** `scripts/uiaudit.sh` captures every screen but checks
   none. Commit a half-resolution set under `docs/screens/` and fail on a pixel
   delta over a threshold. Done when a change that moves a panel fails the
   check until the set is updated.

## Open questions

- Workspaces as a floating bar at the top centre, or a rail down the left
  gutter carrying workspaces and tools together.
- Document tabs in the top bar, or an icon column beside the tool rail with the
  filename on the status pill.

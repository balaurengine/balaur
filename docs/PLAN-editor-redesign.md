# Plan: the editor shell's look

What is left of the shell's craft pass, against the screen catalogue in
[EDITOR-SCREENS.md](EDITOR-SCREENS.md).

## Steps

1. **Keys a drag moves.** `editor/scripts/timeline.bs` places each key as a
   dot button (`editor/scenes/items/track_key.toml`) whose press only seeks.
   Let a drag along its lane move its time. Done when a dragged key's new
   time is in the clip, one undo puts it back, and a self-test drags one.
2. **Asset cards that show the asset.** A card in the Assets panel carries an
   icon, a name and an ink for its type (`dock::assets_view` in
   `editor/scripts/dock.bs`). Give an image file's card its thumbnail. Done
   when a texture's card shows its picture.
3. **Golden screens.** `scripts/uiaudit.sh` captures every screen but checks
   none. Commit a half-resolution set under `docs/screens/` and fail on a pixel
   delta over a threshold. Done when a change that moves a panel fails the
   check until the set is updated.

## Open questions

- Workspaces as a floating bar at the top centre, or a rail down the left
  gutter carrying workspaces and tools together.
- Document tabs in the top bar, or an icon column beside the tool rail with the
  filename on the status pill.

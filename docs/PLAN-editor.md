# Plan: editor tools

The editor tools still to build. Each is a workspace tool or a dock registered
like the built-ins, and each lands with a `--state` self-test.

## Steps

1. **Drop an asset on a node or a property.** A card dragged out of the Assets
   dock lands only in the viewport (`dock::drop_card` in
   `editor/scripts/dock.bs`, through `dropin::place_file`). Let a tree row take
   it as a child or a component, and an inspector property that names an asset
   take its path. Done when both drops go through history and a self-test
   makes each.
2. **Curve editor and onion skin.** See below.

### Curve editor and onion skin

The Animate workspace's timeline shows keys as dots on a lane. A curve view
under it draws each track's channels as curves against time, with a
segment's easing edited by dragging a handle at the key. The named easings
(`animation::ease_names`) stay the storage: a drag picks the nearest one, so
the file stays readable. Onion skin ghosts the posed rig at the previous and
next keys behind the viewport, from the sampler the preview uses. Done when a
handle drag changes a key's easing through history, and a self-test shoots
both.

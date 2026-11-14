# Plan: turning a component off

A component can be switched off on a node, keeping its data while its system
skips it. Today only the components that declare their own `enabled` property
can be: `body2d`, `body3d`, `collider2d`, `collider3d`, `joint2d`, `joint3d`,
`modifier2d`, `modifier3d`, `softbody2d`, `softbody3d`, `state_machine`,
`tile_collision` and `widget`.

## Steps

1. **`Disabled` beside `Attached`.** A second bitset per node in
   `balaur_core::components`, one bit per definition in registration order.
   `components::set_enabled(eng, entity, name, on)` flips it and calls the
   definition's `apply` or `remove` for state held outside the ECS (a rapier
   body, an audio voice); `components::enabled` reads it; `remove_present`
   clears both. Done: a fixture component in `balaur_core`'s tests switches
   off and on with its properties unchanged.
2. **The systems ask.** Rendering, physics, audio, animation and the widget
   layer skip a component whose bit is set, one query filter each. Done: a
   test per system shows a disabled component drawing, simulating or sounding
   nothing.
3. **The scene key.** `disabled = ["sprite", "body3d"]` on the node, read by
   the loader after its components apply and written back by the scene writer.
   Done: a scene round-trips the key.
4. **The inspector's switch.** The heading-row toggle in
   `editor/scripts/inspector.bs` writes the bit through `set_enabled` for
   every section, not only those that declare `enabled`. A disabled section
   draws its rows dimmed.
5. **The own `enabled` properties.** Each is documented as what its system
   does while the component is on, so a disabled `body3d` is off whatever its
   property says.

## Open questions

- Disabling a component another one `expects` (`character2d` expects
  `collider2d`): refuse as a removal does, or only warn.

# Plan: appearance as a component

`transform` is a component with its own `[nodes.transform]` table. The node's
look is not: `visible`, `tint`, `z_index` and `z_as_relative` are still keys at
the node (`NODE_KEYS` in `crates/balaur_core/src/project.rs`), read into the
`Appearance` struct.

## Steps

1. **The component.** Registered in `balaur_core` beside `transform`, with the
   four properties as its schema; `apply` and `get` read and write the
   `Appearance` struct the node bundle already carries, so propagation and its
   readers are untouched. Done when `NODE_KEYS` lists none of the four.
2. **Every scene file.** The four keys move under the component's table in
   every `.toml` under `examples/`, `editor/` and the crates' fixtures, in one
   pass, with no alias for the old keys.

## Open questions

1. **Whether it moves at all.** It breaks every scene file once more.
   `NODE_KEYS` also holds `process` and `tags`, so a node keeps keys outside
   components either way. The owner decides before step 1.

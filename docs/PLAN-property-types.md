> **Status:** done 2026-09-21. Written from the question "why is `nodes` a
> datatype and not an array of nodes, and what happens to a map or a custom
> class?".

# Plan: properties that hold other properties

## 0. Where the tree was

- `PROPERTY_TYPES` was fourteen flat names, two of which were list shapes
  somebody needed once: `strings` and `nodes`. Fifteen schema lines declared
  `strings`; no schema anywhere declared `nodes`.
- `check_default` matched one name at one level, so no type could hold
  another. An array of anything but strings, a map of any kind, and an object
  had no spelling.
- A script's bare export inferred its type from the default's own type
  (`speed: 2.0` is a float). A list or an object fell through to `"string"`
  and the schema then refused the default, so neither could be exported at
  all.
- The two passes over a finished property table, `expand_colors` and
  `resolve_assets`, read the top-level `type` of each property. A colour or an
  asset anywhere else was untouched.
- The host resolved `node` and `nodes` exports with two hand-written branches
  over the top level of the value.
- The inspector looked a drawer up by datatype and fell back to a text field.
  `strings`, `nodes` and `vec4` all drew an empty field, and submitting it
  wrote a bare string over the list.

Everything under the schema already carried nesting. `to_toml`/`from_toml`,
the neutral `Value`, `to_neutral`/`to_plain`/`from_neutral`, the digest and
`toml::patch`'s `as_item` are all recursive and needed no change.

## 1. Design

**Three composites, each naming what it holds in a sibling key.** A spec
already puts a type's extra information beside it: `asset` names its asset
type, `enum` and `flags` take `options`. The composites do the same.

    waves  = { type = "list", of = { type = "int", min = 0 }, default = [] }
    grid   = { type = "list", of = { type = "list", of = { type = "float" } }, default = [] }
    spawns = { type = "map", key = "int", of = { type = "node" }, default = {} }
    wave   = { type = "record", fields = { hp = { type = "int" }, name = { type = "string" } }, default = { hp = 3, name = "boss" } }

- `list` is ordered and uniform, `map` is keyed and uniform, `record` is a
  fixed set of named fields each with a type of its own.
- `of` is the spec of what a `list` or a `map` holds, and `fields` is a table
  of specs, one per `record` field. Each is a whole property spec, so `min`,
  `options`, `asset` and `component` work at any depth with no new keys.
- A nested spec's `default` is what a new entry starts at and may be left out;
  the type's zero is used. A top-level `default` stays required.
- `key` is `"string"` or `"int"` and only a `map` takes one. TOML keys are
  text, so the file spells a number key `"7"`; `"int"` refuses a key that is
  not one, and the script is handed a map keyed by numbers.
- N6 holds: `type` is still the datatype and nothing else declares one.

**`strings` and `nodes` go.** `strings` is `list of string` and `nodes` is
`list of node`, so keeping either would be a second spelling of one thing.
The fifteen schema lines are rewritten and the two names are deleted.

**Inference covers the bare export.** A script writing `waves: [1, 2, 3]`
gets `list of int` from the first entry, with the rest checked against it; an
object gets a `record` whose fields are inferred one by one. An empty list or
a mixed one cannot be inferred and says so, naming the spelling to write
instead.

**A class a script declares is a `record` naming it.** `class = "Wave"` sits
beside `type = "record"` the way `asset` names an asset type, and the host
hands the script an instance of that struct rather than a look-alike object,
so `this.wave.power()` works. The fields are the class's, the file holds a
table, and the inspector draws a row a field. A field the scene leaves out
takes the class's own default.

Rune keeps no types on a struct's fields, its own serde refuses to serialize
one, and everything that can read or build one is `pub(crate)`, so the fork
carries two purpose-built host calls: `Value::struct_parts` reads a struct's
item path and fields, and `Unit::new_struct` builds one from fields by name.

## 2. Steps

1. **The vocabulary.** `PROPERTY_TYPES` loses `strings` and `nodes`, gains
   `list`, `map` and `record`. Done.
2. **Validation recurses.** `validate_property` checks `of`, `fields` and
   `key` where they belong and refuses them elsewhere; `check_default` walks a
   default against the spec tree. A nested spec is validated by the same
   function, so one rule holds at every depth. Done.
3. **The value passes recurse.** `Facts` walks the spec tree, so `has_color`,
   `has_asset` and `has_record` are true for one at any depth, and
   `expand_colors` and `resolve_assets` walk the value beside its spec. A
   third pass fills in the `record` fields a scene left out, so a table naming
   one of two still reaches `apply` and a script whole. Done.
4. **The fifteen schemas.** Every `type = "strings"` becomes
   `list of string`. Done.
5. **Exports.** `export_type` infers `list` and `record` from a bare default,
   and the host resolves node paths by walking the spec tree rather than its
   two branches. Done.
6. **The inspector.** Drawers for `list`, `map` and `record`: a row saying
   what it holds and how many, then a row per entry drawn by the drawer its
   own spec asks for, with a button to take one out and one to add. Every
   drawer takes a cursor (`comp`, `prop`, `path`) instead of a property name,
   so a write reaches into the value and hands the property back whole. Done.
7. **A class that round-trips, and a map keyed by numbers.** The fork gains
   `Value::struct_parts`, `Unit::new_struct` and a public map constructor,
   which are the three things a host cannot do from outside the crate. An
   export reads its spec from the Rune value rather than its plain form, so a
   class says which it is, and the record branch builds that class back. Done.

## 3. What a class does not do

A class held in a script's own state, rather than in an export, is still
dropped from a snapshot: `to_plain` has no spec to tell it which class a value
is, and a recording has to be plain data. An export is the path that knows.

## 4. What is not planned

- **A heterogeneous list.** A row has to know which drawer to put in it, so a
  list holds one type. A mixed sequence is a `record` with named fields.
- **Animating an entry.** `patch_at` addresses a property, not a path inside
  one, so a list entry is not a keyframe target.
- **A key type beyond int and string.** TOML keys are text and everything else
  would be an encoding nobody can read in the file.

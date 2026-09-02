> **Status:** phases 1 and 2 built on 2026-09-02 — `exports` on the host, the
> table form of the `script` scene key, `props` merged before `init`,
> `script::exports` for tools, and the inspector's Properties rows with undo
> and sparse writes. `examples/hello`'s spinner is the worked example: the
> `const SPEED` this plan was written to replace is now an export. Phase 3
> (the Rune fork, `#[export]`) is not started.
>
> **Where the implementation decided differently:**
>
> 1. **A property the script does not export is written, not refused.**
>    §"Open questions" left this open; the scene-key precedent decided it —
>    an unregistered scene key warns and is skipped, but skipping a property
>    would lose an edit, so this one warns and is applied. `balaur export`
>    validating `props` against the declaration stays phase 3's job.
> 2. **`exports` reports a type, and `int` is one of them.** The inspector
>    needs to know which editor to draw, and `PROPERTY_TYPES` has no `int` —
>    but an export declared `2` that came back `2.0` would be a different
>    value to the script and to the file. So `script::exports` names the type
>    itself, the editor drags it as a float, and the write rounds back.
> 3. **Declaration order is not recoverable.** Rune objects do not keep
>    insertion order, so `exports` comes back sorted by name, which is also
>    the order the inspector lists and the encoder writes.
> 4. **`ScriptHost::attach` kept its arity.** `attach_with_props` is the
>    method a backend implements and `attach` defaults to it with no
>    properties, so the forty-odd call sites that never had any did not have
>    to say so.
> 5. **The editor reads game scripts through the host after all.** The plan
>    for the editor (§3, note 5) says game sources are parsed rather than
>    asked about, because the editor's host is rooted at the editor project.
>    `exports` cannot be parsed — it is a function body — so the binding
>    takes the absolute path the way `attach_script` already does, and the
>    editor caches the answer beside the file's text.

# Plan: exported script properties

A field a script marks shows up in the inspector, is stored in the scene,
and is set on the instance before `init` runs.

## Where the tree is today

- A node's `script` scene key is a path. Nothing about a script is data
  except the file it names.
- The editor already lists a file's hooks through `script::functions` and
  reads components through the registry; script state is invisible to it.
- The host hands every instance an empty `this` and calls `init`. Tuning a
  value means editing the file and saving, which hot reloads — fast, but
  not per node, and not in the inspector.

## Design

**Phase one, no fork.** A script declares its properties as a function
returning defaults:

```rune
pub fn exports() {
    #{ speed: 2.0, jumps: 2, color: "#ffcc00", target: "" }
}
```

The `script` scene key grows a table form beside the string form:

```toml
script = "scripts/enemy.rn"
# or
script = { source = "scripts/enemy.rn", props = { speed = 3.5 } }
```

At attach, the host calls `exports()` once per file, merges the node's
`props` over the defaults, writes each onto `this`, then calls `init`. The
inspector's Script section lists the exports with an editor per value type
— the type is the default's type: float, int, bool, string, and a string
that names a node path or an asset shows the picker the component editors
already use. An edit writes `props`; `props` holds only what differs from
the defaults, so a changed default flows to every node that did not
override it. Everything is scene data, so digests, packs and replays are
untouched.

**Phase two, the fork.** Rune has no user attributes. A fork adds one
spelling that lowers to the same table:

```rune
#[export] pub const SPEED = 2.0;
#[export(node)] pub const TARGET = "";
```

so a property is declared next to its use, and the compiler, not a
convention, rejects a typo in `props`. The engine reads the lowered table,
so phase one's host and inspector do not change.

## Phases

1. `props` on the `script` scene key; host merges and writes before `init`;
   `exports()` convention; a test that a prop set in the scene is on `this`
   in `init`, and that a pack round-trips it.
2. Inspector: Script properties section with typed editors, undo through
   `history`, `props` written sparsely.
3. The fork: `#[export]` lowering to the table; `balaur export` validates
   `props` keys against the declaration.

## Open questions

1. **Typed exports without a fork.** A default of `0.0` is a float and `0`
   an int; a string is a string unless the attribute says node or asset.
   Phase one accepts the ambiguity; phase two removes it.
2. **Hot reload of the defaults.** A saved file with a changed default
   applies to nodes without an override on the next attach. Whether a
   running instance's field is rewritten is the same question `hot_reload`
   answers today: it is not, unless the script says so.

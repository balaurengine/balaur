> **Status:** the first two phases shipped on 2026-09-02 — `exports()` on the
> host, the table form of the `script` scene key, `props` merged before
> `init`, `script::exports` for tools, and the inspector's Properties rows
> with undo and sparse writes. `examples/hello`'s spinner is the worked
> example. What is left is the fork.

# Plan: `#[export]` on a script constant

Rune has no user attributes. A fork adds one spelling that lowers to the same
table `exports()` returns today:

```rune
#[export] pub const SPEED = 2.0;
#[export(node)] pub const TARGET = "";
```

so a property is declared next to its use, and the compiler — not a
convention — rejects a typo in `props`. The engine reads the lowered table, so
neither the host nor the inspector changes.

It also removes the one ambiguity the convention accepts. A default of `0.0`
is a float and `0` an int; a string is a string unless something says it names
a node or an asset. `exports()` infers that from the default's type and
`script::exports` reports it; an attribute would state it.

The step is the fork plus one validation: `balaur export` checking `props`
keys against the declaration. Today a property the script does not export is
written with a warning rather than refused, because skipping it would lose an
edit — with a compiler-checked declaration the export can refuse it instead.

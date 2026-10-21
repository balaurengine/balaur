# balaur_port

Rune to Balaur Script, for the editor's style: step 5 of
`docs/PLAN-engine-in-bs.md`. The output is a draft a person reads and fixes.
Whatever the translator could not map is a `// TODO(port): why` line in the
`.bs` and a line in that file's report.

```sh
cargo run -p balaur_port -- editor --out /tmp/editor-bs
```

- Every `.rn` named, or under a directory named, is ported in one pass, so a
  call from one file to another is typed from both ends.
- Each file lands at its path under `--out` as `.bs`, with `.port.md` beside
  it. `SUMMARY.md` has the totals and the flags by kind.
- Engine names and result types come from `docs/generated/api.json`, found
  above the input or given with `--api`.
- Without `--out`: `.bs` on stdout, reports on stderr.
- Parsing is Rune 0.14's own parser (`rune::parse::parse_all`), not regexes.

## Coverage

2026-10-20, editor at `7ce824b2` plus the working tree.

| | files | code lines | clean | flagged |
| --- | ---: | ---: | ---: | ---: |
| `editor/` | 161 | 32,671 | 31,356 (96.0%) | 1,315 (4.0%) |
| `examples/` | 43 | 2,727 | 2,605 (95.5%) | 122 (4.5%) |

- A code line is one that is not blank and not only a comment. It is flagged
  when any `TODO(port)` points at it; clean otherwise.
- Every file ports; none panics; it takes 5 s for the editor in a debug
  build. All 204 outputs parse with `balaur_bs`'s grammar plus the syntax
  below that the compiler does not have yet.
- Clean is not the same as right. 1,130 of the clean lines pass a struct
  (`S` in 1,259 of 1,353 cases) to a function that writes to it. Rune shared
  the object; a `.bs` struct is a value, so the write is lost. Each is a
  `value-struct` note in the report, not a flag: whether `S` becomes a
  reference struct, a global or an `any` is the plan's open question 2, not a
  translation.

What is flagged in the editor: 1,581 flags on its 1,315 flagged lines.

| kind | count | what |
| --- | ---: | --- |
| `capture` | 598 | a closure that read locals, now a `fn` value that cannot |
| `param-type` | 262 | a parameter nothing typed (mostly the generated gamend SDK) |
| `format-spec` | 105 | `{:.2}` and the rest; `.bs` interpolation has none |
| `attempt` | 81 | `script::attempt`: a closure, and catching what it throws |
| `field-type` | 80 | a struct field nothing typed |
| `optional` | 79 | a field read on an optional struct without a check first |
| `copy-write` | 50 | a write through `let h = S.history`, a copy in `.bs` |
| `any-to-struct` | 45 | an `any` where a struct or array is wanted |
| `map` | 45 | removing from, or listing the values of, an `any` |
| `method` | 44 | a method `.bs` lacks (`split_once`, `count`), or on no type |
| others | 192 | `interp`, `type-test`, `chars`, `array`, `computed-key`, … |

## The mapping

| Rune | Balaur Script |
| --- | --- |
| `Some(x)`, `None`, `()` | `x` (into a `T?`), `nil` |
| `match o { Some(v) => .., None => .. }`, `if let` | `if o.is_some() { let v = o.unwrap(); .. } else { .. }` |
| `let v = match o { Some(v) => v, None => return }` | `if o.is_none() { return; }` then `let v = o.unwrap();` |
| `if is_nil(x) { return; }`, `if !(x is Object) { return; }` | the same, then `let x = x.unwrap();` |
| `match s { "a" => .., _ => .. }` | `if s == "a" { .. } else { .. }` |
| `(a, b)`, `let (a, b) = f()` | a `_0`/`_1` struct, then `let a = t._0;` |
| `#{ .. }` | a struct, named from where it sits (`let row`, `rows.push`, `history:`, `fn state`) |
| `#{ .. }` read by computed key, handed to the engine, a property bag | `map().with("k", v)`, an `any` |
| `this.x` in `fn f(this, ..)` | `self.x` in `impl File { fn f(..) }`, fields on `pub struct File` |
| `exports()` | `pub` fields with defaults |
| `xs.iter().any(\|x\| ..)`, `filter`, `map`, `find`, `count`, `collect` | a helper with the loop, called with the captures |
| any other closure | a named function, its captures as leading parameters, flagged |
| `format!("{} x", a)`, `` `${a} x` `` | `"{a} x"`; literal braces doubled |
| `for (i, x) in xs.iter().enumerate()` | `for i in 0..xs.len() { let x = xs[i]; .. }` |
| `for (k, v) in map` | `for k in m.keys() { let v = m.get(k); .. }` |
| `s[a..b]`, `s.chars()` | `s.slice(a, b)`; an index loop over one-byte slices, flagged |
| `use crate::util;`, `crate::util::f()` | `use util;`, `util::f()` |
| `x is Tuple` | `x.is_none()` (`x.is_nil()` on an `any`) |
| `x is Object` | `x.kind() == "map"` on an `any`; on a typed value, its answer |
| `x as f64`, `x as i64` | `x as float`, `x as int`; `.as_float()` on an `any` |
| a `const` that is not a literal | a function of that name |
| `/* .. */` | `// ..` lines |

### Types

Rune has none, so every one is inferred, over every file at once:

- From literals, operators, call sites (both ways), returns, field writes,
  `push`, and the engine's documented signatures and hooks.
- An `any` argument types nothing: the call site reads it out
  (`.as_float()`) as the parameter's other callers type it. A parameter only
  ever handed `any` is `any`.
- An option written to a field or passed to a parameter was checked first,
  or Rune would have failed on it: the field holds what is inside.
- `util::max(a, b)` and the like return their arguments: a call's result is
  typed from its own, so ints stay ints.
- Two passes: the second starts from the first's plain types, so an early
  guess does not merge two structs for good.
- A struct becomes `any` when: it is read by a computed key and never by
  name; the engine's `any` fills it more often than literals build it; it is
  handed on as an `any`; it is wide and each literal names under 40% of it (a
  widget's properties); it sits inside one of those. The report says which.
- Every `S` is the editor's one state struct, declared in `editor.bs`.

### Syntax assumed before the compiler has it

- `T?`, `nil`, `.is_some()`, `.is_none()`, `.unwrap()` (`cx::UNWRAP`),
  `.unwrap_or(d)`.
- `use util;` and `util::f()`; another file's types as `editor::State`, its
  literals as `inspector::Row { .. }`.
- Function values by name, typed `fn(any)`, `fn(str) -> bool`; a field holding
  one is called through a local: `let run1 = row.run; run1()`.
- A struct literal names every field, as `balaur_bs` requires; a field a
  literal left out gets its zero value. Field defaults in literals would
  shorten the larger ones.

## Tests

`tests/snippets.rs`: one per mapping above, on snippets in the editor's
style. `tests/editor.rs`: the whole editor ports, every report adds up, and
at least 80% of its lines are clean.

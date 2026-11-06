---
title: Language reference
description: "Every construct of Balaur Script on one page: types, operators, strings, lists, maps, optionals, any, structs, classes, enums, functions, modules and async."
image: /img/social/scripting.png
sidebar_position: 120
---

# Language reference

Every construct on one page, for looking things up. The rest of this chapter teaches them with examples.

## A file

A `.bs` file holds items in any order: `fn`, `struct`, `class`, `enum`, `impl`, `const`, `extern fn` and
`use`. An item with no mark belongs to its file, `internal` opens it to its package, and `pub` to every file. `//` starts a comment and `///` documents an item.

## Types

| Type | Holds | Assigning it |
| --- | --- | --- |
| `int` | a 64-bit signed integer | copies |
| `float` | a 64-bit float | copies |
| `bool` | `true` or `false` | copies |
| `str` | UTF-8 text that never changes | shares |
| `Node` | a node of the scene, or none | copies the handle |
| `[T]` | a list | shares |
| `{str: V}`, `{int: V}` | a map, kept in the order keys were first set | shares |
| `T?` | a `T`, or `nil` | as `T` |
| `any` | a value of any kind, read at run time | shares a list or a map |
| `fn(A, B) -> R` | a function, named elsewhere | copies |
| `Vec2`, `Vec3`, `Color`, `Transform2d`, `Transform3d` | numbers with named fields | copies |
| a `struct` | fields | copies, shallowly |
| a `class` | fields and methods | shares the object |
| an `enum` | one of its variants | copies |

## Variables and constants

`let` declares a variable and gives it a value: `let n = 3;`, `let x: float = 3;`. A variable can be assigned
again, and `+=`, `-=`, `*=`, `/=` and `%=` combine. A `let` ends with its block and may hide an earlier one.
A `const` holds a literal, for the file, and another file reads it as `util::NAME`.

## Operators

Tightest first: `as`, then unary `-` and `!`, then `*` `/` `%`, then `+` `-`, then `==` `!=` `<` `<=` `>` `>=`,
then `&&`, then `||`. `&&` and `||` stop at the first side that decides.

## Numbers

- `+`, `-` and `*` on ints wrap past the ends of 64 bits.
- `/` on ints truncates toward zero. Dividing by zero traps, and so does the smallest int divided by `-1`.
- `%` takes the sign of the left side, on ints only. `%` by zero traps.
- An int meets a float as a float: `n / 2.0` is a float, and `let x: float = 3` holds `3.0`.
- `as` converts the rest: `int as float`, `bool as int`, and `float as int`, which truncates. A float past an
  int's range gives the nearest end, and NaN gives `0`.
- Floats follow IEEE 754, with the same bits on every engine.

## Strings

- `"{x}"` writes any value into the text. `{{` and `}}` write braces.
- `\n`, `\t`, `\"`, `\\` and `\u{e9}` are escapes. A literal stays on one line.
- `+` joins two strings, and `==` and `!=` compare them.
- `len` counts bytes. `slice(a, b)` takes byte offsets: one inside a character moves back onto it, and one past the end stops at the end.
- `contains`, `starts_with`, `ends_with`, `find` (or `-1`), `replace`, `split`, `split_once`, `chars`, `trim`,
  `to_upper`, `to_lower`, `parse_int` and `parse_float` (`0` when the text is no number).

## Lists

- `[a, b]`, and `[value; count]` for `count` copies. `[]` takes its type from where it goes.
- `xs[i]` reads, and `xs[i] = v` writes. An index past either end traps.
- `len`, `push`, `pop`, `insert`, `remove`, `extend` and `sort`.
- A list is shared: `let b = a; b.push(1);` changes `a` too.
- `for x in xs` walks it in order. `==` compares by elements.

## Maps

- A key is a `str` or an `int`. A value is never nil: a missing key is.
- `m[k]` reads a `V?`, and `m[k] = v` sets. `m[k] += 1` on a missing key traps.
- `has`, `remove`, `len`, `keys` and `values`, in the order keys were set. A key removed and set again goes to the end.
- A literal takes its type from where it goes: `let m = {"a": 1};` does not compile.
- A `{str: V}` goes into an `any` and comes back out of one.

## Optionals

- `T?` is a `T` or `nil`. `nil` goes where a `T?`, an `any` or a `Node` is wanted.
- `x == nil`, `x != nil` and `x == v` compare.
- `x.or(fallback)` gives the value or the fallback, and `x.unwrap()` gives the value or traps.
- `T??` and `any?` do not exist. A check does not change the type.

## any

- Any typed value goes into an `any` where one is wanted. `{ "k": v }` and `[a, b]` are an `any` map and list there.
- `a["k"]` and `a[i]` read, giving nil when there is nothing. `a["k"] = v` and `a[i] = v` write.
- `kind`, `as_int`, `as_float`, `as_bool`, `as_str`, `as_node`, `len`, `has`, `keys`, `text`, `push`, `insert`, `remove` and `sorted`.
- `x == nil`, `a == b` by value, `a.or(v)`, and `for x in a` on a list.
- An `any` goes into a struct, a list, a map, a `T?` or a `fn` at a typed place. A scalar is read with its `as_` method.

## Structs and classes

```bs
pub struct Label { kind: str = "label", size: int = 3, text: str, tip: str? }

pub class Counter { count: int }

impl Counter {
    pub fn bump() { self.count += 1; }
}
```

- A literal names what differs. A field left out takes its default, or nil when it is optional or an `any`. Any other is named.
- A default is a literal of the field's type.
- `impl` applies to a class, with `self` writable and implicit. A struct takes no `impl`.
- A class holds itself through `C?` or a list, and a struct only through a list.
- `==` compares by fields, for both.

## Enums and match

```bs
enum Shape { Circle(float), Rect { w: float, h: float }, Empty }

fn area(s: Shape) -> float {
    match s {
        Shape::Circle(r) => r * r * 3.0,
        Shape::Rect { w, h } => w * h,
        Shape::Empty => 0.0,
    }
}
```

- A variant is made by its path, as declared: `Shape::Circle(2.0)`, `Shape::Rect { w: 1.0, h: 2.0 }`, `Shape::Empty`.
- An arm names a variant and binds its values: by position, by field name, `h: tall` to rename, `_` to bind nothing.
- `_` takes any value, and a bare name takes any value and binds it. Patterns stop at one level, with no guards and no literals.
- A `match` names every variant once, or ends with an arm that takes any value.
- A `match` is a value. An arm may `return`, `break` or `continue`.
- An enum is a value, so one holds itself only through a list or a class.

## Functions

- The last expression of a body, with no `;`, is its value. `return` leaves early.
- A function is a value, stored in a local, a field, a list or an `any`, and called through a local. There are no closures.
- `fn grow(w: &mut Wave)` writes back to the caller's variable or field when it returns. The argument must be a
  variable or a field, and one place is never passed to two `&mut` parameters of one call.
- A trailing `T?` or `any` parameter may be left out, and is nil: an engine function's, a function's, a method's and an `async fn`'s.
  A `&mut` parameter and all before it are given, and a call through a function value gives every argument. There are no variable arguments.
- `balaur check` warns about a last `nil` a call could leave out, unless it is the only argument.

## Control flow

- `if` is a value when both branches give one, and a branch may `return`, `break` or `continue`.
- `while cond { .. }`, `loop { .. }`, `for i in a..b { .. }` and `for x in xs { .. }`, with `break` and `continue`.

## Modules

`use util;` links `util.bs` from beside the file or a folder above it, and its `pub` items read as `util::name`.
`use a::b;` is `a/b.bs`. Files may use each other, and the file that uses them.

Fields and methods take the same marks as items. A package is the `addons/<name>` or `deps/<alias>` folder a file
sits in, or else the project. A name a file may not reach stops at compile time.

## Async

`async fn` may `await` a host call or another script `async fn`, and resumes on a later frame. `await` goes in
a statement, a `let` or an assignment. An `async fn` takes no `&mut` parameter. `update` and `fixed_update` may not be async.

## Errors

A compile error names its line and column: `6:7: cannot combine int and bool`. A file gives one error for each
function that does not compile. A trap ends the call, and the next call runs.

## Not in the language

Generics, traits, inheritance, closures, tuples, exceptions, bit operators, `T??` and `any?`.

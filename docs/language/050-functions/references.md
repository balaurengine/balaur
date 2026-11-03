---
title: References with &mut
description: "&mut parameters in Balaur Script write back to the caller's own variable or field when the function returns, and the rules that keep them simple."
image: /img/social/scripting.png
sidebar_position: 2
---

# References with `&mut`

A struct or a number arrives as a copy. `&mut` before a parameter's type makes it the caller's own
value: changes are written back to the caller's variable or field when the function returns.

```bs
struct Wave { hp: int }

fn grow(w: &mut Wave) { w.hp += 1; }
fn weaken(w: Wave) { w.hp = 0; }
fn inc(n: &mut int) { n += 1; }

pub fn run() -> str {
    let w = Wave { hp: 1 };
    grow(w);
    grow(&mut w);
    weaken(w);
    let n = 1;
    inc(n);
    "{w.hp} {n}"
}

// run() == "3 2"
```

At the call `grow(w)` and `grow(&mut w)` mean the same, and the second is there for
readers who want to see it.

## What can be passed

The argument is a place that can be written to: a variable, or a field of one, `self`'s fields included.

```bs
struct Tally { hits: int }
class Editor { tally: Tally, count: int }

fn bump(t: &mut Tally) { t.hits += 1; }
fn inc(n: &mut int) { n += 1; }

impl Editor {
    pub fn click() -> int {
        bump(self.tally);
        inc(self.count);
        self.tally.hits + self.count
    }
}

pub fn run() -> int {
    let e = Editor { tally: Tally { hits: 0 }, count: 0 };
    e.click()
}

// run() == 2
```

A literal, a call's result or a list element has no place to write back to:

```bs
pub struct I { n: int }
fn bump(i: &mut I) { i.n += 1; }
fn inc(n: &mut int) { n += 1; }

pub fn a() { bump(I { n: 1 }); }
pub fn b() { let xs = [1]; inc(xs[0]); }

// error: 5:19: `bump` changes this argument: pass a variable or a field
// error: 6:34: `inc` changes this argument: pass a variable or a field
```

For an element of a list, read it, pass the local, and write it back. Or write through the index.

## One place, once

One place is never handed to two `&mut` parameters of one call, since the second write would
undo the first.

```bs
pub struct I { n: int }
fn two(a: &mut I, b: &mut I) {}
pub struct E { a: I }
pub fn f(e: E) { two(e.a, e.a); }

// error: 4:28: `two` would change this twice: it is part of another `&mut` argument
```

## When you do not need it

A class object and a list are shared already, so a plain parameter changes them. `&mut` on one of those
lets the function replace the whole thing.

```bs
class C { n: int }

fn change(c: C) { c.n = 5; }
fn replace(c: &mut C) { c = C { n: 9 }; }

pub fn run() -> str {
    let a = C { n: 1 };
    change(a);
    let first = a.n;
    replace(a);
    "{first} {a.n}"
}

// run() == "5 9"
```

## Limits

- An `async fn` takes no `&mut` parameter, because its caller runs on while it waits.
- A reference cannot be kept in a field or a variable. It lives for the call.
- A function value's type says so too: `fn(&mut State, str)`.

```bs
pub struct I { n: int }
async fn w(i: &mut I) {}

// error: 2:12: an `async fn` takes no `&mut` parameter: its caller runs on while it waits
```

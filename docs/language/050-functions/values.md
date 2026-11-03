---
title: Function values
description: "Passing functions around in Balaur Script: fn types, storing a function in a list, field or any, and working without closures."
image: /img/social/scripting.png
sidebar_position: 1
---

# Function values

A function is a value. Its name without a call gives the value, and `fn(A, B) -> R` is its type.

```bs
fn double(x: int) -> int { x * 2 }
fn square(x: int) -> int { x * x }
fn apply(f: fn(int) -> int, x: int) -> int { f(x) }

pub fn twice(k: int) -> int {
    let f = double;
    let g: fn(int) -> int = square;
    apply(f, k) + apply(g, k)
}

pub fn table(k: int) -> int {
    let fs = [double, square, double];
    let t = 0;
    for f in fs {
        t += f(k);
    }
    t
}

// twice(5) == 35
// table(3) == 21
```

## Storing one

A function value goes in a local, a list, a map, a field or an `any`. A call goes through a local, so
take the value out first.

```bs
fn square(x: int) -> int { x * x }

pub class Picker { pick: fn(int) -> int }

impl Picker {
    pub fn run(x: int) -> int {
        let f = self.pick;
        f(x)
    }
}

pub fn picked(k: int) -> int {
    let p = Picker { pick: square };
    p.run(k)
}

pub fn from_map() -> int {
    let named: {str: fn(int) -> int} = { "sq": square };
    let f = named["sq"].unwrap();
    f(3)
}

pub fn from_any() -> int {
    let v: any = { "f": square };
    let f: fn(int) -> int = v["f"];
    f(4)
}

// picked(6) == 36
// from_map() == 9
// from_any() == 16
```

## No closures

A function value names a function, and nothing more. It captures no variables, since there are no
closures. Where a callback needs state, pass the state in, as the second function here does:

```bs
struct Tally { sum: int }

fn add(t: &mut Tally, n: int) { t.sum += n; }

fn each(xs: [int], t: &mut Tally, f: fn(&mut Tally, int)) {
    for x in xs {
        f(t, x);
    }
}

pub fn run() -> int {
    let t = Tally { sum: 0 };
    each([1, 2, 3], t, add);
    t.sum
}

// run() == 6
```

The engine calls back by name instead: a [listener](../engine/events.md) names a method.

## What is a value

Only a plain function of the script is. A method, an engine function and an `async fn` are not, and
the types must match exactly.

```bs
pub class S { x: int }
impl S { fn m() -> int { 1 } }
fn one() -> int { 1 }
async fn wait_a_bit() -> int { 1 }

pub fn method() -> int { let g = S.m; 1 }
pub fn not_a_function(x: int) -> int { x(1) }
pub fn wrong_type() -> int { let g: fn(int) -> int = one; 1 }
pub fn async_one() -> int { let g = wait_a_bit; 1 }

// error: 6:34: unknown name `S`
// error: 7:40: `x` is a int, not a function
// error: 8:54: expected fn(int) -> int, got fn() -> int
// error: 9:37: only a script's own plain function can be a `fn` value; `wait_a_bit` is not one
```

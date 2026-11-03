---
title: Strings
description: "str in Balaur Script: interpolation, escapes, joining, byte offsets, and every string method with an example."
image: /img/social/scripting.png
sidebar_position: 4
---

# Strings

A `str` is UTF-8 text in double quotes. A string never changes: its methods give back
new strings. A literal stays on one line, and `\n` writes a line break.

## Interpolation

`{...}` inside a string writes the value of an expression. A string inside the braces keeps
its own quotes. `{{` and `}}` write the braces themselves.

```bs
const NAME = "ball";

pub fn label(hp: int, speed: float, alive: bool) -> str {
    "{NAME}: hp {hp}, speed {speed}, alive {alive}, next {hp + 1}, {{braces}}"
}

pub fn nested(words: [str]) -> str {
    "first is {words[0].to_upper()} of {words.len()}, {"say \"hi\""}"
}

// label(7, 2.0, true) == "ball: hp 7, speed 2.0, alive true, next 8, {braces}"
// nested(["ok"]) == "first is OK of 1, say \"hi\""
```

Every type writes itself, which makes interpolation the way to turn a value into text. A
list or a struct comes out in a JSON-like form, and a nil optional as `nil`.

```bs
struct Point { x: int }

pub fn shown() -> str {
    let some: int? = 3;
    let none: int? = nil;
    "{some} {none} {[1, 2]} {Point { x: 1 }.x} {vec2(1.0, 2.0)}"
}

pub fn whole() -> str {
    let p = Point { x: 1 };
    "{p}"
}

// shown() == "3 nil [1, 2] 1 [1.0, 2.0]"
// whole() == "{\"x\": 1}"
```

## Escapes

| Escape | Writes |
| --- | --- |
| `\n`, `\t` | line break, tab |
| `\"`, `\\` | quote, backslash |
| `\u{e9}` | the code point, in hex |
| `{{`, `}}` | a brace |

```bs
pub fn points() -> str { "caf\u{e9}\ttwo\nlines" }

// points() == "café\ttwo\nlines"
```

## Joining and comparing

`+` joins two strings, and `==` and `!=` compare them. `<` is not defined on strings. A
number has to be interpolated, since `+` does not cross types.

```bs
pub fn joined(a: str, b: str) -> str { a + "-" + b }
pub fn same(a: str, b: str) -> bool { a == b }

// joined("x", "y") == "x-y"
// same("x", "x") == true
```

## Length and offsets

`len()` counts bytes, and an offset is a byte offset. `chars()` splits by character. A string
has no index: `s[0]` does not compile, so use `slice` or `chars`.

```bs
pub fn bytes(s: str) -> int { s.len() }
pub fn letters(s: str) -> int { s.chars().len() }
pub fn cut(s: str, a: int, b: int) -> str { s.slice(a, b) }

// bytes("héllo") == 6
// letters("héllo") == 5
// cut("hello", 1, 3) == "el"
// cut("hello", 3, 99) == "lo"
```

`slice` is forgiving: an offset past the end stops at the end, and one inside a character moves
back onto it.

## Methods

| Method | Answers |
| --- | --- |
| `len()` | the length in bytes |
| `contains(s)`, `starts_with(s)`, `ends_with(s)` | whether the text is there |
| `find(s)` | the byte offset of the first match, or `-1` |
| `slice(a, b)` | the text between two byte offsets |
| `replace(from, to)` | a copy with every match replaced |
| `split(sep)` | a list of the parts |
| `split_once(sep)` | the two sides of the first match, or a list of one |
| `chars()` | a list of one-character strings |
| `trim()` | a copy without whitespace at the ends |
| `to_upper()`, `to_lower()` | a copy in that case, for all of Unicode |
| `parse_int()`, `parse_float()` | the number, or `0` when the text is no number |

```bs
pub fn words(s: str) -> [str] { s.split(" ") }
pub fn shout(s: str) -> str { s.trim().to_upper() }
pub fn whisper(s: str) -> str { s.to_lower() }
pub fn tagged(s: str) -> bool { s.starts_with("#") && s.contains("go") }
pub fn where_is(s: str, t: str) -> int { s.find(t) }
pub fn swap(s: str) -> str { s.replace("cat", "dog") }
pub fn path(s: str) -> str {
    let parts = s.split_once("#");
    if parts.len() == 2 { "{parts[0]}|{parts[1]}" } else { "none" }
}
pub fn sum(s: str) -> float {
    let total = 0.0;
    for part in s.split(",") {
        total += part.parse_float();
    }
    total
}

// words("a bb c") == ["a", "bb", "c"]
// shout("  hi  ") == "HI"
// whisper("ŞTEFAN") == "ştefan"
// tagged("#let's go!") == true
// where_is("hello", "ll") == 2
// where_is("hello", "x") == -1
// swap("cat and cat") == "dog and dog"
// path("a.png#tile#2") == "a.png|tile#2"
// path("a.png") == "none"
// sum("1.5, 2,x") == 3.5
```

`parse_int` and `parse_float` answer `0` for text that is no number. When zero is a real value
as well, check the text first.

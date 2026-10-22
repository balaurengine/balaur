//! `balaur fmt`: the spaces between tokens made the house's, each line
//! indented by the brackets above it, the author's line breaks and comments
//! kept, and every `.bs` in the repository already laid out that way.

use balaur_bs::fmt::format;

fn laid(src: &str) -> String {
    format(src).unwrap()
}

#[test]
fn spaces_are_the_houses() {
    let messy = "pub struct A{x:int,y :[str]}\nfn f(a:int,b: &mut A)->int?{let v=-a*(b.x+2);if !b.y.len()==0{return nil;}v}\n";
    let want = "pub struct A { x: int, y: [str] }\nfn f(a: int, b: &mut A) -> int? { let v = -a * (b.x + 2); if !b.y.len() == 0 { return nil; } v }\n";
    assert_eq!(laid(messy), want);
}

#[test]
fn calls_indexes_ranges_and_lists_keep_tight() {
    let messy = "fn f(xs: [int]) {\n    for i in 0 .. xs.len() { g( xs [i] , [ 1, -2 ] ); }\n    let h: fn(int) -> int = g;\n}\n";
    let want = "fn f(xs: [int]) {\n    for i in 0..xs.len() { g(xs[i], [1, -2]); }\n    let h: fn(int) -> int = g;\n}\n";
    assert_eq!(laid(messy), want);
}

#[test]
fn lines_are_indented_by_the_brackets_above() {
    let messy = "impl A {\nfn f() {\nlet m = map()\n.with(\"a\", [\n1,\n2,\n])\n.with(\"b\", 2);\nif a\n&& b {\nc();\n}\n}\n}\n";
    let want = "impl A {\n    fn f() {\n        let m = map()\n            .with(\"a\", [\n                1,\n                2,\n            ])\n            .with(\"b\", 2);\n        if a\n            && b {\n            c();\n        }\n    }\n}\n";
    assert_eq!(laid(messy), want);
}

#[test]
fn comments_stay_and_blank_lines_shrink() {
    let messy = "\n\nfn f() {\n\n    let a = 1;   // one\n\n\n\n// two\n    let b = [\n        // three\n        a,\n    ];\n\n}\n\n\n";
    let want = "fn f() {\n    let a = 1;   // one\n\n    // two\n    let b = [\n        // three\n        a,\n    ];\n}\n";
    assert_eq!(laid(messy), want);
}

#[test]
fn strings_are_left_as_written() {
    let src = "fn f() -> str {\n    \"{a.get(\"x\")}  spaced  \\u{1f}\"\n}\n";
    assert_eq!(laid(src), src);
}

#[test]
fn a_file_that_does_not_scan_is_an_error() {
    assert!(format("fn f() { let a = 1 $ 2; }").is_err());
    assert!(format("fn f() { \"open }").is_err());
}

// Every `.bs` the repository ships is laid out, and laying it out again
// changes nothing. A failure names the file: `balaur fmt` on it fixes it.
#[test]
fn every_bs_file_in_the_repository_is_laid_out() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![];
    for dir in ["editor", "examples"] {
        collect(&root.join(dir), &mut files);
    }
    assert!(files.len() > 100, "found {} files", files.len());
    let mut off = vec![];
    for file in files {
        let src = std::fs::read_to_string(&file).unwrap();
        let once = format(&src).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        assert_eq!(
            laid(&once),
            once,
            "{} lays out differently twice",
            file.display()
        );
        if once != src {
            off.push(file.display().to_string());
        }
    }
    assert!(
        off.is_empty(),
        "not laid out, run `balaur fmt` on: {off:#?}"
    );
}

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "bs") {
            out.push(path);
        }
    }
}

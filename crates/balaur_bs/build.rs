// `jit`: wasmtime is in this build. The `wasmtime` feature asks for it, and
// it is granted only where a JIT may run: not in a browser, not on iOS.
fn main() {
    // What a build kept between runs was compiled by (`stored.rs`): any
    // change to the compiler's source makes a kept build stale.
    println!("cargo::rerun-if-changed=src");
    println!(
        "cargo::rustc-env=BALAUR_BS_SOURCE={:016x}",
        source_hash("src")
    );
    println!("cargo::rustc-check-cfg=cfg(jit)");
    println!("cargo::rustc-check-cfg=cfg(native)");
    println!("cargo::rerun-if-env-changed=CARGO_FEATURE_WASMTIME");
    println!("cargo::rerun-if-env-changed=CARGO_FEATURE_NATIVE");
    let asked = std::env::var_os("CARGO_FEATURE_WASMTIME").is_some();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if asked && arch != "wasm32" && os != "ios" {
        println!("cargo::rustc-cfg=jit");
    }
    // `native`: a loaded native build may run a module. A browser cannot open
    // a library, so there the feature is granted nothing.
    if std::env::var_os("CARGO_FEATURE_NATIVE").is_some() && arch != "wasm32" {
        println!("cargo::rustc-cfg=native");
    }
    case_tables();
    mark_tables();
}

/// Rust's own case mapping, as the tables the prelude's `to_upper` and
/// `to_lower` search (`src/prelude/str.bs`): a record of twelve hex digits for
/// a character that becomes one, the code point then what it becomes, and of
/// twenty-five for one that becomes several, with their count between.
fn case_tables() {
    use std::fmt::Write;
    let mut out = String::from(
        "// Written by build.rs from Rust's `char::to_uppercase` and `to_lowercase`.\n",
    );
    for (name, upper) in [("UPPER", true), ("LOWER", false)] {
        let (mut one, mut many) = (String::new(), String::new());
        for character in (0..=0x0010_ffff_u32).filter_map(char::from_u32) {
            let to: Vec<char> = if upper {
                character.to_uppercase().collect()
            } else {
                character.to_lowercase().collect()
            };
            if to == [character] {
                continue;
            }
            let code = u32::from(character);
            if let [single] = to[..] {
                let _ = write!(one, "{code:06x}{:06x}", u32::from(single));
                continue;
            }
            assert!(
                to.len() <= 3,
                "{character:?} maps to more than three characters"
            );
            let _ = write!(many, "{code:06x}{:x}", to.len());
            for slot in 0..3 {
                let mapped = to.get(slot).map_or(0, |became| u32::from(*became));
                let _ = write!(many, "{mapped:06x}");
            }
        }
        let _ = writeln!(out, "const {name}: str = \"{one}\";");
        let _ = writeln!(out, "const {name}_MANY: str = \"{many}\";");
    }
    let dir = std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR");
    std::fs::write(std::path::Path::new(&dir).join("case.bs"), out).expect("OUT_DIR is writable");
}

/// Unicode's own data as the tables the prelude's `without_marks` and
/// `split_words` search (`src/prelude/str.bs`), from the icu crates: the
/// ranges of combining marks (`Mn`), dropped; what each other character
/// becomes once decomposed (NFD) less its marks, as `case_tables` writes a
/// mapping, Hangul's syllables aside (the prelude takes them apart by
/// arithmetic); and the ranges of letters and digits (`L`, `N`) past ASCII.
/// A range is twelve hex digits, its first code point then its last.
fn mark_tables() {
    use icu_normalizer::DecomposingNormalizerBorrowed;
    use icu_properties::CodePointMapData;
    use icu_properties::props::{GeneralCategory, GeneralCategoryGroup};
    use std::fmt::Write;
    let decompose = DecomposingNormalizerBorrowed::new_nfd();
    let category = CodePointMapData::<GeneralCategory>::new();
    let mark = |character: char| category.get(character) == GeneralCategory::NonspacingMark;
    let word = |character: char| {
        let found = category.get(character);
        GeneralCategoryGroup::Letter.contains(found) || GeneralCategoryGroup::Number.contains(found)
    };
    let hangul = 0xac00..=0xd7a3_u32;
    let (mut one, mut many) = (String::new(), String::new());
    let mut marks: Vec<(u32, u32)> = Vec::new();
    let mut words: Vec<(u32, u32)> = Vec::new();
    for character in (0x80..=0x0010_ffff_u32).filter_map(char::from_u32) {
        let code = u32::from(character);
        for (ranges, inside) in [(&mut marks, mark(character)), (&mut words, word(character))] {
            if !inside {
                continue;
            }
            match ranges.last_mut() {
                Some((_, last)) if *last + 1 == code => *last = code,
                _ => ranges.push((code, code)),
            }
        }
        if mark(character) || hangul.contains(&code) {
            continue;
        }
        let mut buffer = [0; 4];
        let kept: Vec<char> = decompose
            .normalize(character.encode_utf8(&mut buffer))
            .chars()
            .filter(|part| !mark(*part))
            .collect();
        if kept == [character] {
            continue;
        }
        if let [single] = kept[..] {
            let _ = write!(one, "{code:06x}{:06x}", u32::from(single));
            continue;
        }
        assert!(
            (2..=3).contains(&kept.len()),
            "{character:?} keeps {} characters",
            kept.len()
        );
        let _ = write!(many, "{code:06x}{:x}", kept.len());
        for slot in 0..3 {
            let kept_code = kept.get(slot).map_or(0, |part| u32::from(*part));
            let _ = write!(many, "{kept_code:06x}");
        }
    }
    let ranges = |list: &[(u32, u32)]| {
        list.iter().fold(String::new(), |mut out, (first, last)| {
            let _ = write!(out, "{first:06x}{last:06x}");
            out
        })
    };
    let out = format!(
        "// Written by build.rs from Unicode's data in the icu crates.\n\
         const MARKS: str = \"{}\";\n\
         const UNMARKED: str = \"{one}\";\n\
         const UNMARKED_MANY: str = \"{many}\";\n\
         const WORD_CHARS: str = \"{}\";\n",
        ranges(&marks),
        ranges(&words)
    );
    let dir = std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR");
    std::fs::write(std::path::Path::new(&dir).join("marks.bs"), out).expect("OUT_DIR is writable");
}

// FNV-1a over every file under `dir`, by path, in a fixed order.
fn source_hash(dir: &str) -> u64 {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = vec![];
    walk(std::path::Path::new(dir), &mut files);
    files.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for file in files {
        let bytes = std::fs::read(&file).unwrap_or_default();
        for b in file.to_string_lossy().bytes().chain(bytes) {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

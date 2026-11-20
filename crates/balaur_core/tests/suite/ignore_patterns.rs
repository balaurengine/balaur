//! `application/ignore` patterns: what `*` and `**` span, and which paths a
//! pattern takes.

use balaur_core::ignore::{from_manifest, glob_matches, matching};

#[test]
fn a_single_star_stays_inside_one_segment() {
    assert!(glob_matches("art/*.png", "art/hero.png"));
    assert!(glob_matches("*", ""));
    assert!(!glob_matches("art/*.png", "art/wip/hero.png"));
    assert!(!glob_matches("*", "a/b"));
}

#[test]
fn a_double_star_crosses_segments() {
    assert!(glob_matches("art/**", "art/wip/hero.png"));
    assert!(glob_matches("**.blend1", "art/wip/hero.blend1"));
    assert!(!glob_matches("art/**", "music/theme.ogg"));
}

#[test]
fn a_double_star_slash_also_stands_for_no_directory() {
    assert!(glob_matches("**/*.blend1", "hero.blend1"));
    assert!(glob_matches("**/*.blend1", "art/wip/hero.blend1"));
    assert!(glob_matches("art/**/hero.png", "art/hero.png"));
    assert!(glob_matches("art/**/hero.png", "art/a/b/hero.png"));
    assert!(!glob_matches("**/*.blend1", "hero.blend"));
}

#[test]
fn everything_but_a_star_is_compared_literally() {
    assert!(glob_matches("a?b[c]", "a?b[c]"));
    assert!(!glob_matches("a?b", "axb"));
    assert!(!glob_matches("abc", "ab"));
    assert!(!glob_matches("ab", "abc"));
}

#[test]
fn a_pattern_naming_a_directory_takes_everything_under_it() {
    let patterns = from_manifest("[application]\nignore = [\"art/wip/\"]\n");
    assert_eq!(matching(&patterns, "art/wip/hero.png"), Some("art/wip/"));
    assert_eq!(matching(&patterns, "art/hero.png"), None);
    assert_eq!(matching(&patterns, "tests/a.bs"), Some("tests/**"));
}

#[test]
fn a_pattern_of_many_stars_that_cannot_match_answers_at_once() {
    let text = "v".repeat(4096);
    for pattern in ["*********T", "**********T", "*a*a*a*a*a*a*a*a*b"] {
        assert!(!glob_matches(pattern, &text));
    }
    assert!(glob_matches("*********v", &text));
}

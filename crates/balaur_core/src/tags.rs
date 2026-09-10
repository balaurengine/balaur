//! The names this run answers to, and the only vocabulary an override is
//! written against.
//!
//! A tag is a fact about where the game is running: the kind of machine, the
//! operating system, the architecture, the build. `[override.android]` in
//! `project.toml` is read when `android` is one of these, which is what lets
//! one project carry two answers to the same setting. See
//! [`crate::settings`] for how an override is stored and resolved.
//!
//! Tags are derived from the same facts a recording restores, so a session
//! recorded on a phone replays against the phone's settings rather than the
//! desktop's.

/// The prefix an override is stored under: `override/<tag>/<path>`.
pub const OVERRIDE: &str = "override";

/// A machine somebody sits at.
pub const DESKTOP: &str = "desktop";
/// A machine somebody holds.
pub const MOBILE: &str = "mobile";

pub const WINDOWS: &str = "windows";
pub const MACOS: &str = "macos";
pub const LINUX: &str = "linux";
pub const ANDROID: &str = "android";
pub const IOS: &str = "ios";
/// Both the operating system and the kind of machine: a page is neither a
/// desktop nor a phone, and a game that cares asks for `web`.
pub const WEB: &str = "web";

pub const X86_64: &str = "x86_64";
pub const ARM64: &str = "arm64";
pub const WASM32: &str = "wasm32";

/// A build with assertions on: the editor's own runs, and a game exported
/// against a debug template.
pub const DEBUG: &str = "debug";
pub const RELEASE: &str = "release";

/// The tags in force, broad to narrow: the kind of machine, the operating
/// system, the architecture, the build, then whatever a target added.
///
/// The order is the precedence. Two overrides on one key are resolved by
/// taking the later tag, so `[override.android]` outranks `[override.mobile]`
/// however they sit in the file.
#[derive(Clone, Debug)]
pub struct Tags(pub Vec<String>);

impl Default for Tags {
    fn default() -> Self {
        Self::current()
    }
}

impl Tags {
    /// What this build, on this machine, answers to.
    #[must_use]
    pub fn current() -> Self {
        let mut tags = Vec::new();
        if let Some(group) = group() {
            tags.push(group.to_string());
        }
        tags.push(os().to_string());
        tags.push(arch().to_string());
        tags.push(if cfg!(debug_assertions) { DEBUG } else { RELEASE }.to_string());
        Self(tags)
    }

    /// Add a name a target declared, which outranks every derived tag.
    pub fn push(&mut self, tag: &str) {
        if !self.has(tag) {
            self.0.push(tag.to_string());
        }
    }

    #[must_use]
    pub fn has(&self, tag: &str) -> bool {
        self.0.iter().any(|held| held == tag)
    }

    /// Narrowest first, which is the order an override is looked for in.
    pub fn narrowest_first(&self) -> impl Iterator<Item = &str> {
        self.0.iter().rev().map(String::as_str)
    }

    /// What an export target answers to, for a build that resolves a
    /// project's settings for a machine other than the one exporting.
    ///
    /// A target names an operating system and often an architecture
    /// (`linux-arm64`); a universal binary names two, so it claims neither.
    /// Every export is a release build.
    #[must_use]
    pub fn for_target(target: &str) -> Self {
        let (os, arch) = target.split_once('-').unwrap_or((target, ""));
        let os = match os {
            "macos" => MACOS,
            "windows" => WINDOWS,
            "android" => ANDROID,
            "ios" => IOS,
            "web" => WEB,
            _ => LINUX,
        };
        let mut tags = Vec::new();
        if os == ANDROID || os == IOS {
            tags.push(MOBILE.to_string());
        } else if os != WEB {
            tags.push(DESKTOP.to_string());
        }
        tags.push(os.to_string());
        match arch {
            "x64" => tags.push(X86_64.to_string()),
            "arm64" => tags.push(ARM64.to_string()),
            _ if os == WEB => tags.push(WASM32.to_string()),
            _ => {}
        }
        tags.push(RELEASE.to_string());
        Self(tags)
    }
}

/// The kind of machine, or `None` for the web, where the operating system tag
/// already says it.
#[must_use]
pub fn group() -> Option<&'static str> {
    if cfg!(target_family = "wasm") {
        None
    } else if cfg!(any(target_os = "ios", target_os = "android")) {
        Some(MOBILE)
    } else {
        Some(DESKTOP)
    }
}

/// The operating system, spelled as `PlatformFacts::os` spells it.
#[must_use]
pub fn os() -> &'static str {
    if cfg!(target_family = "wasm") {
        WEB
    } else {
        match std::env::consts::OS {
            "windows" => WINDOWS,
            "macos" => MACOS,
            "android" => ANDROID,
            "ios" => IOS,
            _ => LINUX,
        }
    }
}

/// The architecture, in the spelling an export target writes: `arm64`, not
/// the `aarch64` the compiler calls it.
#[must_use]
pub fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "wasm32" => WASM32,
        "aarch64" | "arm64" => ARM64,
        _ => X86_64,
    }
}

#[cfg(test)]
mod tests {
    use super::{Tags, arch, os};

    #[test]
    fn a_run_answers_to_its_machine_and_its_build() {
        let tags = Tags::current();
        assert!(tags.has(os()), "{tags:?}");
        assert!(tags.has(arch()), "{tags:?}");
        assert!(
            tags.has(super::DEBUG) || tags.has(super::RELEASE),
            "{tags:?}"
        );
    }

    /// The narrow tag is offered first, so `android` beats `mobile` whatever
    /// order the file wrote them in.
    #[test]
    fn the_narrowest_tag_is_looked_for_first() {
        let mut tags = Tags(vec!["mobile".into(), "android".into()]);
        tags.push("demo");
        let order: Vec<&str> = tags.narrowest_first().collect();
        assert_eq!(order, ["demo", "android", "mobile"]);
    }

    #[test]
    fn a_tag_a_target_added_is_not_added_twice() {
        let mut tags = Tags(vec!["demo".into()]);
        tags.push("demo");
        assert_eq!(tags.0.len(), 1);
    }
}

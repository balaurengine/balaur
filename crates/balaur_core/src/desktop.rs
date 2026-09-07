//! The shell around a running game: handing it a URL to open, or a file to
//! show in the file manager.
//!
//! Three shells, because each hands work over differently. A desktop spawns an
//! opener process. A browser tab calls `window.open`. A phone has neither and
//! wants its own platform call, which is not wired up yet.
//!
//! Both are effects on the world outside the simulation and neither is
//! recorded, the way rumble is not: a replay must not open a browser window,
//! and nothing about the result feeds a later tick.
//!
//! The tools are each platform's own, called with arguments rather than
//! through a shell, so nothing here interpolates a string into a command line.

use anyhow::{Result, bail};

/// The desktop shell: an opener process and a file manager. Gated once here
/// rather than on each of the five items inside, which a phone and a tab both
/// lack.
#[cfg(not(any(target_family = "wasm", target_os = "ios", target_os = "android")))]
mod shell {
    use std::path::Path;
    use std::process::Command;

    use anyhow::{Result, bail};

    pub(super) fn hand_over(url: &str) -> Result<()> {
        spawn(&opener(), &[url])
    }

    /// Show a file in the file manager, selected where the platform can.
    ///
    /// # Errors
    /// If the path does not exist, or the OS has no file manager to ask.
    pub fn reveal(path: &Path) -> Result<()> {
        if !path.exists() {
            bail!("{} does not exist", path.display());
        }
        let text = path.to_string_lossy().into_owned();
        if cfg!(target_os = "macos") {
            return spawn("open", &["-R", &text]);
        }
        if cfg!(windows) {
            return spawn("explorer", &[&format!("/select,{text}")]);
        }
        // Nothing portable selects a file on a Linux desktop, so the directory
        // holding it is what opens.
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        spawn(&opener(), &[&dir.to_string_lossy()])
    }

    fn opener() -> String {
        if cfg!(target_os = "macos") {
            "open".into()
        } else if cfg!(windows) {
            "explorer".into()
        } else {
            "xdg-open".into()
        }
    }

    /// Hand the work over and stop caring: a file manager runs for as long as
    /// the player leaves it open, which is not something a frame waits for.
    fn spawn(program: &str, args: &[&str]) -> Result<()> {
        Command::new(program)
            .args(args)
            .spawn()
            .map_err(|e| anyhow::anyhow!("{program}: {e}"))?;
        Ok(())
    }
}

#[cfg(not(any(target_family = "wasm", target_os = "ios", target_os = "android")))]
use shell::hand_over;
/// Revealing a file needs a file manager, which only a desktop has, so this
/// exists on a desktop alone.
#[cfg(not(any(target_family = "wasm", target_os = "ios", target_os = "android")))]
pub use shell::reveal;

/// Schemes a game may hand the OS. A URL becomes whatever the system has
/// registered for its scheme, so the set is closed rather than open: `file:`
/// opens a local path and every custom scheme is some other program's entry
/// point.
const OPENABLE: [&str; 3] = ["http://", "https://", "mailto:"];

/// Open a URL in whatever the player browses with.
///
/// Every desktop and a browser tab can. iOS and Android cannot yet; the
/// error says so.
///
/// # Errors
/// If the scheme is not one a game may open, or this shell has no way to
/// open it.
pub fn open_url(url: &str) -> Result<()> {
    if !OPENABLE.iter().any(|scheme| url.starts_with(scheme)) {
        bail!("open_url takes an http, https or mailto URL, not {url:?}");
    }
    hand_over(url)
}

/// A phone has no opener process to spawn: iOS wants `UIApplication.openURL:`
/// and Android an `ACTION_VIEW` intent, each through that platform's own
/// crate. Saying so beats spawning `xdg-open`, which no phone has, and
/// reporting that it is missing.
#[cfg(any(target_os = "ios", target_os = "android"))]
fn hand_over(url: &str) -> Result<()> {
    bail!("open_url has no opener on this platform yet, so {url:?} stays closed")
}

/// A new tab, which is what a browser has instead of an opener. A popup
/// blocker may refuse one that no click led to, and says so by handing back
/// nothing.
#[cfg(target_family = "wasm")]
fn hand_over(url: &str) -> Result<()> {
    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no window to open from"))?;
    match window.open_with_url_and_target(url, "_blank") {
        Ok(Some(_)) => Ok(()),
        Ok(None) => bail!("the browser refused to open {url:?}; a popup blocker wants a click"),
        Err(_) => bail!("the browser could not open {url:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::open_url;

    #[test]
    fn only_the_schemes_a_game_may_open_are_opened() {
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "steam://run/440",
            "not a url at all",
        ] {
            let err = open_url(refused).unwrap_err().to_string();
            assert!(err.contains("http"), "{refused}: {err}");
        }
    }
}

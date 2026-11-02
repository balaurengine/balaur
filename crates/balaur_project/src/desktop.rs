//! The shell around a running game: handing it a URL to open, or a file to
//! show in the file manager.
//!
//! Three shells, because each hands work over differently. A desktop asks the
//! system's own handler (`opener`), a phone its browser (`webbrowser`), and a
//! browser tab calls `window.open`.
//!
//! Both are effects on the world outside the simulation and neither is
//! recorded, the way rumble is not: a replay must not open a browser window,
//! and nothing about the result feeds a later tick.
//!

use anyhow::{Result, bail};

/// The desktop shell, through `opener`: the system's own handler for a URL,
/// and the file manager. Gated once here rather than on each item inside,
/// which a phone and a tab both lack.
#[cfg(desktop)]
mod shell {
    use std::path::Path;

    use anyhow::{Result, bail};

    pub(super) fn hand_over(url: &str) -> Result<()> {
        opener::open(url).map_err(|e| anyhow::anyhow!("opening {url:?}: {e}"))
    }

    /// Show a file in the file manager, selected where the platform can.
    ///
    /// # Errors
    /// If the path does not exist, or the OS has no file manager to ask.
    pub fn reveal(path: &Path) -> Result<()> {
        if !path.exists() {
            bail!("{} does not exist", path.display());
        }
        opener::reveal(path).map_err(|e| anyhow::anyhow!("revealing {}: {e}", path.display()))
    }
}

#[cfg(desktop)]
use shell::hand_over;
/// Revealing a file needs a file manager, which only a desktop has, so this
/// exists on a desktop alone.
#[cfg(desktop)]
pub use shell::reveal;

/// Schemes a game may hand the OS. A URL becomes whatever the system has
/// registered for its scheme, so the set is closed rather than open: `file:`
/// opens a local path and every custom scheme is some other program's entry
/// point.
const OPENABLE: [&str; 3] = ["http://", "https://", "mailto:"];

/// Open a URL in whatever the player browses with: on a desktop, a phone or
/// a browser tab.
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

/// A phone's browser through `webbrowser`: `UIApplication.openURL:` on iOS,
/// an `ACTION_VIEW` intent on Android.
#[cfg(mobile)]
fn hand_over(url: &str) -> Result<()> {
    webbrowser::open(url).map_err(|e| anyhow::anyhow!("opening {url:?}: {e}"))
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

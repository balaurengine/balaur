//! Exporting to a platform whose game is a directory rather than a file: the
//! iOS `.app`, the Android APK layout, the web directory a static host
//! serves, and the signed macOS `.app`.
//!
//! Split out of `main.rs`, which keeps the command line itself.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::apple::{AppleConfig, Platform};
use crate::roots_for_message;

/// A platform whose game is a directory the OS launches, not a file it runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bundle {
    /// An `.app`, with the pack beside the executable inside it.
    Ios,
    /// An APK layout, with the pack under `assets/`.
    Android,
    /// A directory for a static host: the shell page, the wasm and its
    /// glue, and the pack beside them.
    Web,
}

impl Bundle {
    pub(crate) fn for_target(target: &str) -> Option<Self> {
        match target {
            "ios" => Some(Self::Ios),
            "android" => Some(Self::Android),
            "web" => Some(Self::Web),
            _ => None,
        }
    }

    /// The template directory `package_template.sh` produces.
    const fn template_dir(self) -> &'static str {
        match self {
            Self::Ios => "Balaur.app",
            Self::Android => "balaur-template-android",
            Self::Web => "balaur-template-web",
        }
    }

    pub(crate) const fn platform(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
            Self::Web => "web",
        }
    }
}

/// The page a web export ships, unless the project has `web/index.html` of
/// its own. `{{title}}` and `{{pack}}` are filled in.
const WEB_SHELL: &str = include_str!("web/index.html");

/// The shell for a project: its own, or the built-in one.
pub(crate) fn web_shell(project: &Path) -> Result<String> {
    let own = project.join("web").join("index.html");
    if own.is_file() {
        return std::fs::read_to_string(&own).with_context(|| format!("reading {}", own.display()));
    }
    Ok(WEB_SHELL.to_string())
}

/// Replace what an earlier export wrote, and refuse anything else: `-o .`
/// would otherwise delete the working directory before writing into it.
fn replace_export(dir: &Path, pack_inside: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let empty = std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none());
    anyhow::ensure!(
        dir.join(pack_inside).exists() || empty,
        "{} already exists and is not an export of this game; \
         move it aside or name another output with -o",
        dir.display()
    );
    std::fs::remove_dir_all(dir).with_context(|| format!("replacing {}", dir.display()))
}

/// Copy a bundle template and put the pack where that platform looks for it.
pub(crate) fn export_bundle(
    kind: Bundle,
    template: &Path,
    pack: &[u8],
    name: &str,
    output: Option<PathBuf>,
    apple: &AppleConfig,
    shell: &str,
) -> Result<PathBuf> {
    if kind == Bundle::Ios {
        apple.check(Platform::Ios)?;
    }
    let output = output.unwrap_or_else(|| match kind {
        Bundle::Ios => PathBuf::from(format!("{name}.app")),
        Bundle::Android => PathBuf::from(format!("{name}-android")),
        Bundle::Web => PathBuf::from(format!("{name}-web")),
    });
    let inside = match kind {
        Bundle::Ios | Bundle::Web => PathBuf::from(balaur::standalone::BUNDLED_PACK),
        Bundle::Android => Path::new("assets").join(balaur::standalone::BUNDLED_PACK),
    };
    if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    replace_export(&output, &inside)?;
    copy_dir(template, &output)?;
    let pack_path = match kind {
        Bundle::Ios | Bundle::Web => output.join(balaur::standalone::BUNDLED_PACK),
        Bundle::Android => {
            let assets = output.join("assets");
            std::fs::create_dir_all(&assets)?;
            assets.join(balaur::standalone::BUNDLED_PACK)
        }
    };
    std::fs::write(&pack_path, pack).with_context(|| format!("writing {}", pack_path.display()))?;
    if kind == Bundle::Web {
        let page = shell
            .replace("{{title}}", name)
            .replace("{{pack}}", balaur::standalone::BUNDLED_PACK);
        let index = output.join("index.html");
        std::fs::write(&index, page).with_context(|| format!("writing {}", index.display()))?;
        tracing::info!(
            "exported for the web -> {} (serve the directory; the page fetches the pack beside it)",
            output.display()
        );
        return Ok(output);
    }
    if kind == Bundle::Ios {
        let plist = output.join("Info.plist");
        let executable = template_executable(&plist).unwrap_or_else(|| "Balaur".to_string());
        std::fs::write(&plist, apple.info_plist(Platform::Ios, &executable, name))
            .with_context(|| format!("writing {}", plist.display()))?;
        if let Some(path) = apple.write_entitlements(&output, name)? {
            tracing::info!(
                "entitlements -> {} (codesign --entitlements {} --sign <identity> {})",
                path.display(),
                path.display(),
                output.display()
            );
        }
    }
    tracing::info!("exported for {} -> {}", kind.platform(), output.display());
    Ok(output)
}

/// The binary inside the template bundle, read off the plist the exporter is
/// about to replace: the executable file keeps the template's name, so the
/// new plist has to name the same one.
fn template_executable(plist: &Path) -> Option<String> {
    let text = std::fs::read_to_string(plist).ok()?;
    let after = text.split("<key>CFBundleExecutable</key>").nth(1)?;
    let open = after.find("<string>")? + "<string>".len();
    let close = after.find("</string>")?;
    Some(after[open..close].to_string())
}

pub(crate) fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("creating {}", to.display()))?;
    for entry in
        std::fs::read_dir(from).with_context(|| format!("reading template {}", from.display()))?
    {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
            // The executable inside an .app has to stay executable, and a
            // template that came through an artifact store has already lost
            // the bit once.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(entry.path())?.permissions().mode();
                if mode & 0o111 != 0 {
                    std::fs::set_permissions(
                        &target,
                        std::fs::Permissions::from_mode(mode | 0o755),
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// A macOS game as a signable `.app`: the template binary untouched, the
/// pack a resource beside it (`standalone::own_pack` looks there inside a
/// bundle), and `codesign` run over the result.
pub(crate) fn export_macos_app(
    template: &Path,
    pack: &[u8],
    name: &str,
    output: Option<PathBuf>,
    sign: Option<&str>,
    apple: &AppleConfig,
) -> Result<PathBuf> {
    apple.check(Platform::Macos)?;
    let app = output.unwrap_or_else(|| PathBuf::from(format!("{name}.app")));
    if let Some(dir) = app.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let macos_dir = app.join("Contents").join("MacOS");
    let resources = app.join("Contents").join("Resources");
    replace_export(
        &app,
        &Path::new("Contents")
            .join("Resources")
            .join(balaur::standalone::BUNDLED_PACK),
    )?;
    std::fs::create_dir_all(&macos_dir)?;
    std::fs::create_dir_all(&resources)?;
    let bytes = std::fs::read(template)
        .with_context(|| format!("reading template {}", template.display()))?;
    balaur::standalone::write_executable(&macos_dir.join(name), &bytes, template)?;
    std::fs::write(resources.join(balaur::standalone::BUNDLED_PACK), pack)?;
    std::fs::write(
        app.join("Contents").join("Info.plist"),
        apple.info_plist(Platform::Macos, name, name),
    )?;
    let entitlements = apple.write_entitlements(&app, name)?;
    crate::sign::codesign(&app, sign, entitlements.as_deref(), true)?;
    if sign.is_some() {
        crate::sign::verify(&app)?;
    }
    tracing::info!(
        "exported {} ({} signature)",
        app.display(),
        match sign {
            Some(identity) => identity,
            None if cfg!(target_os = "macos") => "ad-hoc",
            None => "no",
        }
    );
    Ok(app)
}

/// Find the bundle template for a mobile platform.
pub(crate) fn find_bundle_template(kind: Bundle, roots: &[PathBuf]) -> Result<PathBuf> {
    for root in roots {
        let candidate = root.join(kind.template_dir());
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "no {} template (looked for {} in: {}). Unpack balaur-template-{} from the \
         release into the templates directory, or pass --template <dir>.",
        kind.platform(),
        kind.template_dir(),
        roots_for_message(roots),
        kind.platform(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `-o .` names a directory full of someone's work; only a directory this
    /// exporter wrote may be replaced.
    #[test]
    fn replacing_refuses_a_directory_that_is_not_an_export() {
        let dir = tempfile::tempdir().unwrap();
        let occupied = dir.path().join("Documents");
        std::fs::create_dir(&occupied).unwrap();
        std::fs::write(occupied.join("thesis.txt"), b"years of work").unwrap();

        let err = replace_export(&occupied, Path::new(balaur::standalone::BUNDLED_PACK))
            .unwrap_err()
            .to_string();

        assert!(err.contains("not an export"), "{err}");
        assert!(occupied.join("thesis.txt").exists());
    }

    #[test]
    fn replacing_removes_an_earlier_export_of_this_game() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("game.app");
        std::fs::create_dir(&app).unwrap();
        std::fs::write(app.join(balaur::standalone::BUNDLED_PACK), b"pack").unwrap();

        replace_export(&app, Path::new(balaur::standalone::BUNDLED_PACK)).unwrap();

        assert!(!app.exists());
    }

    #[test]
    fn a_web_export_is_the_template_the_pack_and_a_page_that_names_both() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("balaur-template-web");
        std::fs::create_dir(&template).unwrap();
        std::fs::write(template.join("balaur.js"), b"export default 1;").unwrap();
        std::fs::write(template.join("balaur_bg.wasm"), b"\0asm").unwrap();
        let out = dir.path().join("game-web");

        export_bundle(
            Bundle::Web,
            &template,
            b"pack",
            "Tide",
            Some(out.clone()),
            &AppleConfig::default(),
            WEB_SHELL,
        )
        .unwrap();

        assert_eq!(
            std::fs::read(out.join("balaur.js")).unwrap(),
            b"export default 1;"
        );
        assert_eq!(std::fs::read(out.join("balaur_bg.wasm")).unwrap(), b"\0asm");
        assert_eq!(
            std::fs::read(out.join(balaur::standalone::BUNDLED_PACK)).unwrap(),
            b"pack"
        );
        let page = std::fs::read_to_string(out.join("index.html")).unwrap();
        assert!(page.contains("<title>Tide</title>"), "{page}");
        assert!(
            page.contains(&format!("./{}", balaur::standalone::BUNDLED_PACK)),
            "the page fetches the pack beside it: {page}"
        );
        assert!(!page.contains("{{"), "every placeholder was filled: {page}");
    }

    #[test]
    fn a_projects_own_page_wins_over_the_built_in_shell() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("web")).unwrap();
        std::fs::write(dir.path().join("web").join("index.html"), "<p>mine</p>").unwrap();
        assert_eq!(web_shell(dir.path()).unwrap(), "<p>mine</p>");
        let other = tempfile::tempdir().unwrap();
        assert_eq!(web_shell(other.path()).unwrap(), WEB_SHELL);
    }

    #[test]
    fn replacing_accepts_an_empty_directory_the_user_made() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("out");
        std::fs::create_dir(&empty).unwrap();

        replace_export(&empty, Path::new(balaur::standalone::BUNDLED_PACK)).unwrap();

        assert!(!empty.exists());
    }
}

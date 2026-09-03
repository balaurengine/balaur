//! Exporting to a platform whose game is a directory rather than a file: the
//! iOS `.app`, the Android APK layout, and the signed macOS `.app`.
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
}

impl Bundle {
    pub(crate) fn for_target(target: &str) -> Option<Self> {
        match target {
            "ios" => Some(Self::Ios),
            "android" => Some(Self::Android),
            _ => None,
        }
    }

    /// The template directory `package_template.sh` produces.
    const fn template_dir(self) -> &'static str {
        match self {
            Self::Ios => "Balaur.app",
            Self::Android => "balaur-template-android",
        }
    }

    const fn platform(self) -> &'static str {
        match self {
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
}

/// Copy a bundle template and put the pack where that platform looks for it.
pub(crate) fn export_bundle(
    kind: Bundle,
    template: &Path,
    pack: &[u8],
    name: &str,
    output: Option<PathBuf>,
    apple: &AppleConfig,
) -> Result<()> {
    if kind == Bundle::Ios {
        apple.check(Platform::Ios)?;
    }
    let output = output.unwrap_or_else(|| match kind {
        Bundle::Ios => PathBuf::from(format!("{name}.app")),
        Bundle::Android => PathBuf::from(format!("{name}-android")),
    });
    if output.exists() {
        std::fs::remove_dir_all(&output)
            .with_context(|| format!("replacing {}", output.display()))?;
    }
    copy_dir(template, &output)?;
    let pack_path = match kind {
        Bundle::Ios => output.join(balaur::standalone::BUNDLED_PACK),
        Bundle::Android => {
            let assets = output.join("assets");
            std::fs::create_dir_all(&assets)?;
            assets.join(balaur::standalone::BUNDLED_PACK)
        }
    };
    std::fs::write(&pack_path, pack).with_context(|| format!("writing {}", pack_path.display()))?;
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
    tracing::info!(
        "exported for {} -> {} (unsigned; sign it before installing)",
        kind.platform(),
        output.display()
    );
    Ok(())
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
) -> Result<()> {
    apple.check(Platform::Macos)?;
    let app = output.unwrap_or_else(|| PathBuf::from(format!("{name}.app")));
    let macos_dir = app.join("Contents").join("MacOS");
    let resources = app.join("Contents").join("Resources");
    std::fs::remove_dir_all(&app).ok();
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
    codesign(&app, sign, entitlements.as_deref())?;
    tracing::info!(
        "exported {} ({} signature)",
        app.display(),
        match sign {
            Some(identity) => identity,
            None if cfg!(target_os = "macos") => "ad-hoc",
            None => "no",
        }
    );
    Ok(())
}

/// Ad-hoc unless an identity is given. Signing needs Apple's `codesign`, so
/// off macOS the bundle is written unsigned and says so.
pub(crate) fn codesign(app: &Path, sign: Option<&str>, entitlements: Option<&Path>) -> Result<()> {
    if !cfg!(target_os = "macos") {
        if sign.is_some() {
            anyhow::bail!("--sign runs codesign, which needs macOS");
        }
        tracing::warn!("unsigned .app: run codesign over it on a Mac");
        return Ok(());
    }
    let mut command = std::process::Command::new("codesign");
    command.args(["--force", "--sign", sign.unwrap_or("-")]);
    if let Some(entitlements) = entitlements {
        command.arg("--entitlements").arg(entitlements);
    }
    let status = command.arg(app).status().context("running codesign")?;
    anyhow::ensure!(status.success(), "codesign failed");
    Ok(())
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

//! Applying an identity to what the exporter just wrote: `codesign` and the
//! notary service on Apple platforms, Authenticode on Windows.
//!
//! Every signature this engine applies is applied here, by `balaur export`,
//! and never by a script or a CI step beside it: a bug in a signed build has
//! to be reproducible by exporting it by hand. Each vendor's own tool does the
//! work, because each already holds the developer's login where that vendor
//! documents, and reimplementing either means owning a certificate chain.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::config::{ExportConfig, WindowsConfig, secret, secret_or};
use crate::vocabulary::words;

const CODESIGN: &str = "codesign";
const DITTO: &str = "ditto";
const OSSLSIGNCODE: &str = "osslsigncode";
const PRODUCTBUILD: &str = "productbuild";
const SHA256: &str = "sha256";
const ZIP: &str = "zip";

/// Run a signing tool, and fail with what it printed rather than a status.
///
/// The output matters here more than anywhere else in the exporter: an
/// identity that is not in the keychain and a certificate that has expired
/// both come back as "exit 1" and nothing else.
pub(crate) fn run(command: &mut Command, what: &str) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("running {what}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let said = [stdout.trim(), stderr.trim()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        bail!("{what} failed:\n{said}");
    }
    Ok(stdout)
}

/// A tool on `PATH`, or an error naming what installs it.
pub(crate) fn tool(name: &str, install: &str) -> Result<PathBuf> {
    which(name).with_context(|| format!("{name} is not on PATH; {install}"))
}

fn which(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}

/// Sign a macOS or iOS bundle, nested code first.
///
/// A bundle is signed inside out because a signature seals what is under it:
/// signing the outside first and a framework afterwards leaves the outer seal
/// describing bytes that have since changed.
pub(crate) fn codesign(
    bundle: &Path,
    identity: Option<&str>,
    entitlements: Option<&Path>,
    hardened: bool,
) -> Result<()> {
    if !cfg!(target_os = "macos") {
        if identity.is_some() {
            bail!("signing runs codesign, which needs macOS");
        }
        tracing::warn!("unsigned bundle: run codesign over it on a Mac");
        return Ok(());
    }
    for nested in nested_code(bundle) {
        sign_one(&nested, identity, None, hardened)?;
    }
    sign_one(bundle, identity, entitlements, hardened)
}

/// Everything inside a bundle that carries its own signature: the frameworks
/// and loadable libraries an extension or a store SDK puts there, and a
/// `--steam` export's `steam_api` beside the executable.
fn nested_code(bundle: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in [
        "Contents/Frameworks",
        "Frameworks",
        "Contents/PlugIns",
        "Contents/MacOS",
    ] {
        let Ok(entries) = std::fs::read_dir(bundle.join(dir)) else {
            continue;
        };
        let mut here: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    || p.extension().is_some_and(|e| {
                        e == words::artifacts::DYLIB
                            || e == words::artifacts::SO
                            || e == words::artifacts::BUNDLE
                    })
            })
            .collect();
        // Read order is the filesystem's, and a signed bundle has to come out
        // the same whichever machine wrote it.
        here.sort();
        found.extend(here);
    }
    found
}

fn sign_one(
    path: &Path,
    identity: Option<&str>,
    entitlements: Option<&Path>,
    hardened: bool,
) -> Result<()> {
    let mut command = Command::new(CODESIGN);
    command.args(["--force", "--sign", identity.unwrap_or("-")]);
    // Notarization refuses anything without them, and both are free on a
    // build that is never submitted.
    if hardened && identity.is_some() {
        command.args(["--options", "runtime", "--timestamp"]);
    }
    if let Some(entitlements) = entitlements {
        command.arg("--entitlements").arg(entitlements);
    }
    run(command.arg(path), CODESIGN)?;
    Ok(())
}

/// What `codesign --verify --strict` says, as a check an export can fail on.
pub(crate) fn verify(bundle: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    run(
        Command::new(CODESIGN)
            .args(["--verify", "--strict", "--deep"])
            .arg(bundle),
        "codesign --verify",
    )?;
    Ok(())
}

/// Submit a signed bundle to Apple's notary service and staple the ticket.
///
/// The service takes an archive rather than a bundle, so the `.app` is zipped
/// the way Apple's own documentation says to — `ditto`, keeping the parent —
/// and the ticket is stapled onto the original.
pub(crate) fn notarize(bundle: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("--notarize runs xcrun notarytool, which needs macOS");
    }
    let key = secret("BALAUR_APPLE_NOTARY_KEY")?;
    let key_id = secret("BALAUR_APPLE_NOTARY_KEY_ID")?;
    let issuer = secret("BALAUR_APPLE_NOTARY_ISSUER_ID")?;
    let archive = bundle.with_extension("notarize.zip");
    run(
        Command::new(DITTO)
            .args(["-c", "-k", "--keepParent"])
            .arg(bundle)
            .arg(&archive),
        DITTO,
    )?;
    tracing::info!(
        "notarizing {} — Apple's queue decides how long",
        bundle.display()
    );
    let said = run(
        Command::new(words::native::XCRUN)
            .args(["notarytool", "submit"])
            .arg(&archive)
            .args(["--key", &key, "--key-id", &key_id, "--issuer", &issuer])
            .args(["--wait", "--output-format", "json"]),
        "xcrun notarytool submit",
    );
    let _ = std::fs::remove_file(&archive);
    let said = said?;
    if !said.contains("\"status\":\"Accepted\"") && !said.contains("\"status\": \"Accepted\"") {
        bail!("the notary service did not accept the build:\n{said}");
    }
    run(
        Command::new(words::native::XCRUN)
            .args(["stapler", "staple"])
            .arg(bundle),
        "xcrun stapler staple",
    )?;
    tracing::info!("notarized and stapled {}", bundle.display());
    Ok(())
}

/// An `.ipa`: the App Store's shape for an iOS build, which is the `.app`
/// inside a `Payload` directory, zipped. The entry is `<name>.app` even when
/// `--output` named the bundle without the suffix: the App Store and every
/// signer look for `Payload/*.app`.
pub(crate) fn build_ipa(app: &Path, output: &Path) -> Result<PathBuf> {
    let name = app
        .file_name()
        .context("the exported .app has no name")?
        .to_string_lossy()
        .into_owned();
    let bundled = Path::new(&name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("app"));
    let name = if bundled { name } else { format!("{name}.app") };
    let staging = output.with_extension("ipa-staging");
    let payload = staging.join("Payload");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&payload)?;
    crate::bundle::copy_dir(app, &payload.join(&name))?;
    let ipa = output.with_extension("ipa");
    let _ = std::fs::remove_file(&ipa);
    let zip = tool(ZIP, "it ships with macOS and in every Linux base image")?;
    run(
        Command::new(zip)
            .current_dir(&staging)
            .args(["-qr", "-X"])
            .arg(absolute(&ipa)?)
            .arg("Payload"),
        ZIP,
    )?;
    std::fs::remove_dir_all(&staging)?;
    Ok(ipa)
}

/// An installer package, which is what the Mac App Store takes instead of a
/// bundle, installing into `/Applications`: `<output>.pkg`, signed with
/// `installer`, an installer identity rather than the application's.
pub(crate) fn build_pkg(app: &Path, output: &Path, installer: Option<&str>) -> Result<PathBuf> {
    if !cfg!(target_os = "macos") {
        bail!("a .pkg is made by productbuild, which needs macOS");
    }
    let pkg = output.with_extension("pkg");
    run(&mut pkg_command(app, &pkg, installer), PRODUCTBUILD)?;
    Ok(pkg)
}

/// `productbuild --component <app> /Applications [--sign <installer>] <pkg>`.
pub(crate) fn pkg_command(app: &Path, pkg: &Path, installer: Option<&str>) -> Command {
    let mut command = Command::new(PRODUCTBUILD);
    command.arg("--component").arg(app).arg("/Applications");
    if let Some(installer) = installer {
        command.arg("--sign").arg(installer);
    }
    command.arg(pkg);
    command
}

/// Authenticode over a fused Windows executable.
///
/// Signing runs after fusing, because a signature cannot cover bytes appended
/// later; the certificate table lands after the pack and
/// `standalone::extract` reads in front of it.
pub(crate) fn sign_windows(exe: &Path, project: &Path, config: &WindowsConfig) -> Result<()> {
    let timestamp = &config.timestamp_url;
    let certificate = ExportConfig::beside(project, &config.certificate);
    if cfg!(windows) {
        let mut command = Command::new(tool("signtool.exe", "it ships in the Windows SDK")?);
        command.args([
            words::sign::SIGN,
            "/fd",
            SHA256,
            "/tr",
            timestamp,
            "/td",
            SHA256,
        ]);
        match &certificate {
            // Azure Trusted Signing keeps the key in an HSM: since 2023 an OV
            // certificate's key cannot be a file, so this is the common path.
            Some(path)
                if path
                    .extension()
                    .is_some_and(|e| e == words::artifacts::JSON) =>
            {
                command.arg("/dlib").arg(dlib()).arg("/dmdf").arg(path);
            }
            Some(path) => {
                command.arg("/f").arg(path);
                if let Ok(password) = std::env::var("BALAUR_WINDOWS_CERTIFICATE_PASSWORD") {
                    command.args(["/p", &password]);
                }
            }
            None => bail!("[windows] certificate names no certificate to sign with"),
        }
        run(command.arg(exe), "signtool")?;
    } else {
        let certificate =
            certificate.context("[windows] certificate names no certificate to sign with")?;
        let signed = exe.with_extension("signed.exe");
        let mut command = Command::new(tool(
            OSSLSIGNCODE,
            "it is what signs a Windows build from a Linux or macOS runner \
             (apt install osslsigncode, brew install osslsigncode)",
        )?);
        command.args([words::sign::SIGN, "-h", SHA256, "-ts", timestamp]);
        command.arg("-pkcs12").arg(&certificate);
        command.args([
            "-pass",
            &secret_or("BALAUR_WINDOWS_CERTIFICATE_PASSWORD", ""),
        ]);
        run(
            command.arg("-in").arg(exe).arg("-out").arg(&signed),
            OSSLSIGNCODE,
        )?;
        std::fs::rename(&signed, exe)?;
    }
    tracing::info!("signed {}", exe.display());
    Ok(())
}

/// Trusted Signing's signtool plug-in, wherever the developer installed it.
fn dlib() -> String {
    secret_or("BALAUR_WINDOWS_SIGN_DLIB", "Azure.CodeSigning.Dlib.dll")
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if balaur::files::rooted(path) {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir()?.join(path))
}

#[cfg(test)]
mod tests {
    use super::{nested_code, pkg_command, which};

    // `--output /out/game` writes the bundle as `game`, which is how Forge
    // exports; an ipa of it without `.app` is one no signer can find. Unix
    // only: Windows runners have no `zip`.
    #[cfg(unix)]
    #[test]
    fn an_ipa_holds_a_dot_app_whatever_the_bundle_was_called() {
        use super::build_ipa;

        let work = tempfile::tempdir().unwrap();
        let app = work.path().join("game");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("Info.plist"), "<plist/>").unwrap();

        let ipa = build_ipa(&app, &app).unwrap();
        assert_eq!(ipa, work.path().join("game.ipa"));

        let listing = std::process::Command::new("unzip")
            .arg("-Z1")
            .arg(&ipa)
            .output()
            .unwrap();
        let listing = String::from_utf8(listing.stdout).unwrap();
        assert!(
            listing
                .lines()
                .any(|line| line == "Payload/game.app/Info.plist"),
            "{listing}"
        );
    }

    #[test]
    fn a_bundle_signs_what_it_contains_before_itself() {
        let work = tempfile::tempdir().unwrap();
        let app = work.path().join("Game.app");
        for dir in ["Contents/Frameworks", "Contents/PlugIns", "Contents/MacOS"] {
            std::fs::create_dir_all(app.join(dir)).unwrap();
        }
        for file in [
            "Contents/Frameworks/b.dylib",
            "Contents/Frameworks/a.dylib",
            "Contents/Frameworks/notes.txt",
            "Contents/PlugIns/one.bundle",
            "Contents/MacOS/Game",
            "Contents/MacOS/steam.dylib",
        ] {
            std::fs::write(app.join(file), []).unwrap();
        }
        std::fs::create_dir_all(app.join("Contents/Frameworks/Store.framework")).unwrap();
        let found: Vec<String> = nested_code(&app)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            found,
            [
                "Store.framework",
                "a.dylib",
                "b.dylib",
                "one.bundle",
                "steam.dylib"
            ],
            "sorted within a directory, the frameworks first, the executable not among them"
        );
        assert!(
            !found.iter().any(|name| name == "notes.txt"),
            "a file that carries no signature is not signed"
        );
    }

    // What App Store Connect takes: the bundle installed into /Applications,
    // signed with the installer identity when there is one.
    #[test]
    fn a_pkg_installs_the_bundle_into_applications_signed_by_the_installer() {
        let installer = "3rd Party Mac Developer Installer: Studio (AB12CD34EF)";
        let command = pkg_command(
            std::path::Path::new("out/t.app"),
            std::path::Path::new("out/t.pkg"),
            Some(installer),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--component",
                "out/t.app",
                "/Applications",
                "--sign",
                installer,
                "out/t.pkg"
            ]
        );
        let unsigned = pkg_command(
            std::path::Path::new("t.app"),
            std::path::Path::new("t.pkg"),
            None,
        );
        assert!(!unsigned.get_args().any(|arg| arg == "--sign"));
    }

    #[test]
    fn a_tool_that_is_not_installed_is_not_found() {
        assert!(which("balaur-no-such-tool-anywhere").is_none());
    }
}

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

use crate::config::{ExportConfig, secret, secret_or};

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
    let path = std::env::var_os("PATH")?;
    let exact = Path::new(name);
    if exact.components().count() > 1 {
        return exact.is_file().then(|| exact.to_path_buf());
    }
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(name);
        candidate.is_file().then_some(candidate)
    })
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
/// and loadable libraries an extension or a store SDK puts there.
fn nested_code(bundle: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in ["Contents/Frameworks", "Frameworks", "Contents/PlugIns"] {
        let Ok(entries) = std::fs::read_dir(bundle.join(dir)) else {
            continue;
        };
        let mut here: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    || p.extension()
                        .is_some_and(|e| e == "dylib" || e == "so" || e == "bundle")
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
    let mut command = Command::new("codesign");
    command.args(["--force", "--sign", identity.unwrap_or("-")]);
    // Notarization refuses anything without them, and both are free on a
    // build that is never submitted.
    if hardened && identity.is_some() {
        command.args(["--options", "runtime", "--timestamp"]);
    }
    if let Some(entitlements) = entitlements {
        command.arg("--entitlements").arg(entitlements);
    }
    run(command.arg(path), "codesign")?;
    Ok(())
}

/// What `codesign --verify --strict` says, as a check an export can fail on.
pub(crate) fn verify(bundle: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    run(
        Command::new("codesign")
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
    let key = secret("BALAUR_NOTARY_KEY")?;
    let key_id = secret("BALAUR_NOTARY_KEY_ID")?;
    let issuer = secret("BALAUR_NOTARY_ISSUER_ID")?;
    let archive = bundle.with_extension("notarize.zip");
    run(
        Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(bundle)
            .arg(&archive),
        "ditto",
    )?;
    tracing::info!(
        "notarizing {} — Apple's queue decides how long",
        bundle.display()
    );
    let said = run(
        Command::new("xcrun")
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
        Command::new("xcrun")
            .args(["stapler", "staple"])
            .arg(bundle),
        "xcrun stapler staple",
    )?;
    tracing::info!("notarized and stapled {}", bundle.display());
    Ok(())
}

/// An `.ipa`: the App Store's shape for an iOS build, which is the `.app`
/// inside a `Payload` directory, zipped.
pub(crate) fn build_ipa(app: &Path, output: &Path) -> Result<PathBuf> {
    let name = app
        .file_name()
        .context("the exported .app has no name")?
        .to_owned();
    let staging = output.with_extension("ipa-staging");
    let payload = staging.join("Payload");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&payload)?;
    crate::bundle::copy_dir(app, &payload.join(&name))?;
    let ipa = output.with_extension("ipa");
    let _ = std::fs::remove_file(&ipa);
    let zip = tool("zip", "it ships with macOS and in every Linux base image")?;
    run(
        Command::new(zip)
            .current_dir(&staging)
            .args(["-qr", "-X"])
            .arg(absolute(&ipa)?)
            .arg("Payload"),
        "zip",
    )?;
    std::fs::remove_dir_all(&staging)?;
    Ok(ipa)
}

/// An installer package, which is what the Mac App Store takes instead of a
/// bundle. Signed with an installer identity, not the application's.
pub(crate) fn build_pkg(app: &Path, output: &Path, identity: Option<&str>) -> Result<PathBuf> {
    if !cfg!(target_os = "macos") {
        bail!("--pkg runs productbuild, which needs macOS");
    }
    let pkg = output.with_extension("pkg");
    let mut command = Command::new("productbuild");
    command.arg("--component").arg(app).arg("/Applications");
    if let Some(identity) = identity {
        command.arg("--sign").arg(installer_identity(identity));
    }
    run(command.arg(&pkg), "productbuild")?;
    Ok(pkg)
}

/// The Mac App Store wants the *installer* certificate, whose name differs
/// from the application one by a word, so a project states one and gets both.
fn installer_identity(application: &str) -> String {
    application
        .replace("Apple Distribution", "3rd Party Mac Developer Installer")
        .replace("Developer ID Application", "Developer ID Installer")
}

/// Authenticode over a fused Windows executable.
///
/// Signing runs after fusing, because a signature cannot cover bytes appended
/// later; the certificate table lands after the pack and
/// `standalone::extract` reads in front of it.
pub(crate) fn sign_windows(exe: &Path, project: &Path, config: &ExportConfig) -> Result<()> {
    let timestamp = &config.windows_timestamp_url;
    let certificate = ExportConfig::beside(project, &config.windows_certificate);
    if cfg!(windows) {
        let mut command = Command::new(tool("signtool.exe", "it ships in the Windows SDK")?);
        command.args(["sign", "/fd", "sha256", "/tr", timestamp, "/td", "sha256"]);
        match &certificate {
            // Azure Trusted Signing keeps the key in an HSM: since 2023 an OV
            // certificate's key cannot be a file, so this is the common path.
            Some(path) if path.extension().is_some_and(|e| e == "json") => {
                command.arg("/dlib").arg(dlib()).arg("/dmdf").arg(path);
            }
            Some(path) => {
                command.arg("/f").arg(path);
                if let Ok(password) = std::env::var("BALAUR_SIGN_PASSWORD") {
                    command.args(["/p", &password]);
                }
            }
            None => bail!("[export] windows_certificate names no certificate to sign with"),
        }
        run(command.arg(exe), "signtool")?;
    } else {
        let certificate = certificate
            .context("[export] windows_certificate names no certificate to sign with")?;
        let signed = exe.with_extension("signed.exe");
        let mut command = Command::new(tool(
            "osslsigncode",
            "it is what signs a Windows build from a Linux or macOS runner \
             (apt install osslsigncode, brew install osslsigncode)",
        )?);
        command.args(["sign", "-h", "sha256", "-ts", timestamp]);
        command.arg("-pkcs12").arg(&certificate);
        command.args(["-pass", &secret_or("BALAUR_SIGN_PASSWORD", "")]);
        run(
            command.arg("-in").arg(exe).arg("-out").arg(&signed),
            "osslsigncode",
        )?;
        std::fs::rename(&signed, exe)?;
    }
    tracing::info!("signed {}", exe.display());
    Ok(())
}

/// Trusted Signing's signtool plug-in, wherever the developer installed it.
fn dlib() -> String {
    secret_or("BALAUR_SIGN_DLIB", "Azure.CodeSigning.Dlib.dll")
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir()?.join(path))
}

#[cfg(test)]
mod tests {
    use super::{installer_identity, which};

    #[test]
    fn an_installer_identity_is_the_application_one_renamed() {
        assert_eq!(
            installer_identity("Apple Distribution: Studio (AB12CD34EF)"),
            "3rd Party Mac Developer Installer: Studio (AB12CD34EF)"
        );
        assert_eq!(
            installer_identity("Developer ID Application: Studio (AB12CD34EF)"),
            "Developer ID Installer: Studio (AB12CD34EF)"
        );
    }

    #[test]
    fn a_tool_that_is_not_installed_is_not_found() {
        assert!(which("balaur-no-such-tool-anywhere").is_none());
    }
}

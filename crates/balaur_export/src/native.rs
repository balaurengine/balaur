// An iOS game's `.bs` scripts as native code: their C, compiled by Xcode's
// clang into a framework in the app for the device or simulator it targets.

use crate::apple::AppleConfig;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

/// What a template's executable was built for, read off its Mach-O header.
#[derive(Debug, PartialEq, Eq)]
struct Slice {
    arch: &'static str,
    simulator: bool,
}

const MH_MAGIC_64: u32 = 0xfeed_facf;
const CPU_ARM64: u32 = 0x0100_000c;
const CPU_X86_64: u32 = 0x0100_0007;
const LC_BUILD_VERSION: u32 = 0x32;
const PLATFORM_IOS_SIMULATOR: u32 = 7;

fn slice_of(bin: &[u8]) -> Option<Slice> {
    let word = |at: usize| Some(u32::from_le_bytes(bin.get(at..at + 4)?.try_into().ok()?));
    if word(0)? != MH_MAGIC_64 {
        return None;
    }
    let arch = match word(4)? {
        CPU_ARM64 => "arm64",
        CPU_X86_64 => "x86_64",
        _ => return None,
    };
    let mut at = 32;
    for _ in 0..word(16)? {
        let (cmd, size) = (word(at)?, word(at + 4)? as usize);
        if cmd == LC_BUILD_VERSION {
            let simulator = word(at + 8)? == PLATFORM_IOS_SIMULATOR;
            return Some(Slice { arch, simulator });
        }
        at += size;
    }
    Some(Slice {
        arch,
        simulator: false,
    })
}

/// `scripts` compiled into `<app>/Frameworks/<framework>.framework`. Off a Mac,
/// or without Xcode, the game keeps running its scripts on the interpreter,
/// and the export says so.
pub(crate) fn build_framework(
    app: &Path,
    executable: &str,
    scripts: &balaur::NativeScripts,
    apple: &AppleConfig,
) -> Result<()> {
    if !cfg!(target_os = "macos") || !has_xcode() {
        tracing::warn!(
            "no Xcode here: the game's {} script(s) run on the interpreter; export on a Mac with \
             Xcode to run them as native code",
            scripts.modules
        );
        return Ok(());
    }
    let bin = std::fs::read(app.join(executable))
        .with_context(|| format!("reading the runtime {executable}"))?;
    let Some(slice) = slice_of(&bin) else {
        bail!("{executable} is not an iOS executable this exporter can read");
    };
    let (sdk, platform, suffix) = if slice.simulator {
        ("iphonesimulator", "iPhoneSimulator", "-simulator")
    } else {
        ("iphoneos", "iPhoneOS", "")
    };
    let framework = scripts.framework;
    let dir = app
        .join("Frameworks")
        .join(format!("{framework}.framework"));
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let work = std::env::temp_dir().join(format!("balaur-native-{}", std::process::id()));
    std::fs::create_dir_all(&work)?;
    let source = work.join(format!("{framework}.c"));
    std::fs::write(&source, &scripts.c).with_context(|| format!("writing {}", source.display()))?;
    let target = format!("{}-apple-ios{}{suffix}", slice.arch, apple.min_ios);
    let out = Command::new("xcrun")
        .args(["--sdk", sdk, "clang", "-target", &target])
        // Contracting a multiply and an add into one rounding would part
        // this build's floats from every other engine's.
        .args(["-O2", "-ffp-contract=off", "-fPIC", "-dynamiclib", "-w"])
        .arg("-install_name")
        .arg(format!("@rpath/{framework}.framework/{framework}"))
        .arg("-o")
        .arg(dir.join(framework))
        .arg(&source)
        .output()
        .context("running Xcode's clang")?;
    let _ = std::fs::remove_dir_all(&work);
    if !out.status.success() {
        bail!(
            "compiling the scripts as native code failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    std::fs::write(
        dir.join("Info.plist"),
        apple.framework_plist(framework, platform)?,
    )?;
    tracing::info!(
        "{} script(s) as native code -> {}",
        scripts.modules,
        dir.display()
    );
    Ok(())
}

fn has_xcode() -> bool {
    Command::new("xcrun")
        .args(["--sdk", "iphoneos", "--show-sdk-path"])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(test)]
mod tests {
    use super::{Slice, slice_of};

    fn header(cpu: u32, platform: Option<u32>) -> Vec<u8> {
        let mut b = vec![];
        for w in [
            super::MH_MAGIC_64,
            cpu,
            0,
            2,
            u32::from(platform.is_some()),
            24,
            0,
            0,
        ] {
            b.extend(w.to_le_bytes());
        }
        if let Some(p) = platform {
            for w in [super::LC_BUILD_VERSION, 24, p, 0, 0, 0] {
                b.extend(w.to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn a_template_says_whether_it_is_for_the_simulator() {
        assert_eq!(
            slice_of(&header(super::CPU_ARM64, Some(7))),
            Some(Slice {
                arch: "arm64",
                simulator: true
            })
        );
        assert_eq!(
            slice_of(&header(super::CPU_ARM64, Some(2))),
            Some(Slice {
                arch: "arm64",
                simulator: false
            })
        );
        assert_eq!(slice_of(b"#!/bin/sh"), None);
    }
}

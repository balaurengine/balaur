//! What App Store Connect requires of an Apple bundle beyond what `[apple]`
//! declares: the keys Xcode writes into an iOS `Info.plist` from the SDK and
//! the Xcode it built with, which an export keeps from the template or reads
//! off the Xcode that built the scripts, and checks of the plists and the
//! signature as an export wrote them, so what the store refuses fails the
//! export rather than the upload.

use anyhow::{Context, Result, bail};

use crate::apple::{AppleConfig, Platform};
use crate::plist::{self, Value};
use crate::vocabulary::words;

pub(crate) const SUPPORTED_PLATFORMS: &str = "CFBundleSupportedPlatforms";
pub(crate) const DEVICE_FAMILY: &str = "UIDeviceFamily";
pub(crate) const REQUIRED_CAPABILITIES: &str = "UIRequiredDeviceCapabilities";
pub(crate) const REQUIRES_FULL_SCREEN: &str = "UIRequiresFullScreen";
pub(crate) const IPAD_ORIENTATIONS: &str = "UISupportedInterfaceOrientations~ipad";
/// What a 64-bit binary requires of the device it installs on.
pub(crate) const ARM64: &str = "arm64";
/// What an iPad that multitasks turns to.
pub(crate) const EVERY_ORIENTATION: [&str; 4] = [
    "UIInterfaceOrientationPortrait",
    "UIInterfaceOrientationPortraitUpsideDown",
    "UIInterfaceOrientationLandscapeLeft",
    "UIInterfaceOrientationLandscapeRight",
];

/// `CFBundleSupportedPlatforms`' platform on a device and on the simulator.
pub(crate) const IPHONE_OS: &str = "iPhoneOS";
pub(crate) const IPHONE_SIMULATOR: &str = "iPhoneSimulator";
/// `DTPlatformName`, the SDK's name, on a device and on the simulator.
pub(crate) const DEVICE_SDK: &str = "iphoneos";
pub(crate) const SIMULATOR_SDK: &str = "iphonesimulator";
const PLATFORM_NAME: &str = "DTPlatformName";
const PLATFORM_VERSION: &str = "DTPlatformVersion";
const BUILD_MACHINE: &str = "BuildMachineOSBuild";
/// The compiler Xcode names, clang's, which builds every bundle here.
const COMPILER: &str = "com.apple.compilers.llvm.clang.1_0";
pub(crate) const EXECUTABLE: &str = "CFBundleExecutable";
pub(crate) const MINIMUM_OS: &str = "MinimumOSVersion";

pub(crate) const APPLICATION_IDENTIFIER: &str = "com.apple.application-identifier";
pub(crate) const TEAM_IDENTIFIER: &str = "com.apple.developer.team-identifier";
/// iOS spells the application identifier without the prefix a Mac's carries.
const IOS_APPLICATION_IDENTIFIER: &str = "application-identifier";
/// Whether a debugger may attach, true only on a development build; a Mac
/// spells it as a hardened runtime exception.
const GET_TASK_ALLOW: &str = "get-task-allow";
const MAC_GET_TASK_ALLOW: &str = "com.apple.security.get-task-allow";
/// What TestFlight's builds carry.
const BETA_REPORTS_ACTIVE: &str = "beta-reports-active";

const COMPILER_KEY: &str = "DTCompiler";
const SDK_BUILD: &str = "DTSDKBuild";
const SDK_NAME: &str = "DTSDKName";
const XCODE: &str = "DTXcode";
const XCODE_BUILD: &str = "DTXcodeBuild";
const XCODEBUILD: &str = "xcodebuild";
const SW_VERS: &str = "sw_vers";

/// The toolchain keys App Store Connect reads, each written by Xcode, and by
/// `scripts/package_runtime.sh` into a template, from the SDK and the Xcode
/// the build used.
const TOOLCHAIN_KEYS: [&str; 8] = [
    BUILD_MACHINE,
    COMPILER_KEY,
    PLATFORM_NAME,
    PLATFORM_VERSION,
    SDK_BUILD,
    SDK_NAME,
    XCODE,
    XCODE_BUILD,
];

/// What every bundle names itself by, which the export writes from `[apple]`.
const BUNDLE_KEYS: [&str; 7] = [
    EXECUTABLE,
    "CFBundleIdentifier",
    "CFBundleName",
    "CFBundlePackageType",
    "CFBundleShortVersionString",
    "CFBundleVersion",
    MINIMUM_OS,
];

/// What an iOS bundle's plist takes from the template it was copied from.
#[derive(Default)]
pub(crate) struct IosTemplate {
    /// The executable the template's plist names.
    pub(crate) executable: Option<String>,
    /// The template plist's `DT*` keys and `BuildMachineOSBuild`, in its
    /// order: every `DT*` one, so a key a later Xcode writes is kept too.
    pub(crate) toolchain: Vec<(String, String)>,
    /// Built for the simulator rather than a device, read off its binary.
    pub(crate) simulator: bool,
    /// The SDK its binary was linked against, from its load commands.
    pub(crate) sdk: Option<(u32, u32)>,
}

impl IosTemplate {
    /// The template whose plist is `plist`, before its executable is read.
    pub(crate) fn read(plist: &str) -> Result<Self> {
        let mut template = Self::default();
        for (key, value) in plist::top_level(plist)? {
            let Some(text) = value.text() else { continue };
            if key == EXECUTABLE {
                template.executable = Some(text.to_string());
            } else if key.starts_with("DT") || key == BUILD_MACHINE {
                template.toolchain.push((key, text.to_string()));
            }
        }
        Ok(template)
    }

    /// The template, with what its executable `binary` was built for and
    /// linked against.
    pub(crate) fn built(self, binary: &[u8]) -> Self {
        Self {
            simulator: crate::native::built_for_simulator(binary),
            sdk: crate::macho::sdk(binary),
            ..self
        }
    }

    /// The one platform `CFBundleSupportedPlatforms` names.
    pub(crate) const fn platform(&self) -> &'static str {
        if self.simulator {
            IPHONE_SIMULATOR
        } else {
            IPHONE_OS
        }
    }
}

/// Which `Info.plist` a check reads: the app's own, or that of the native
/// scripts framework inside it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    App,
    Framework,
}

/// Refuse a device bundle whose `Info.plist`, `text` as written with the
/// project's own keys in it, App Store Connect would refuse, naming every
/// key that is missing or wrong. `template` says what the binary beside it
/// was built for. A simulator's bundle is never uploaded.
pub(crate) fn check(text: &str, template: &IosTemplate, part: Part) -> Result<()> {
    if template.simulator {
        return Ok(());
    }
    let keys = plist::top_level(text)?;
    let value = |name: &str| keys.iter().find(|(key, _)| key == name).map(|(_, v)| v);
    let text_of = |name: &str| value(name).and_then(Value::text);
    let lists = |name: &str, item: &str| value(name).is_some_and(|v| v.lists(item));
    let missing = |keys: &[&'static str]| -> Vec<&'static str> {
        keys.iter()
            .copied()
            .filter(|key| text_of(key).is_none())
            .collect()
    };
    let mut problems = Vec::new();
    let unnamed = missing(&BUNDLE_KEYS);
    if !unnamed.is_empty() {
        problems.push(format!("it names no {}", unnamed.join(", ")));
    }
    let missing = missing(&TOOLCHAIN_KEYS);
    if !missing.is_empty() {
        problems.push(format!(
            "it lacks {}, which Xcode writes from the SDK and the Xcode a build used: {} (90507)",
            missing.join(", "),
            match part {
                Part::App => "build the template again with scripts/package_runtime.sh",
                Part::Framework => "build the scripts with Xcode",
            }
        ));
    }
    if let Some(name) = text_of(PLATFORM_NAME).filter(|name| *name != DEVICE_SDK) {
        problems.push(format!(
            "{PLATFORM_NAME} is {name}, and the binary is a device's: the plist was written for \
             another build"
        ));
    }
    let named = text_of(PLATFORM_VERSION).and_then(crate::apple::version_of);
    // A linker that records no SDK writes 0.0, which names none.
    if let (Some(named), Some(linked)) = (named, template.sdk)
        && linked != (0, 0)
        && named != linked
    {
        problems.push(format!(
            "{PLATFORM_VERSION} names the iOS {}.{} SDK, and the binary was linked against {}.{}",
            named.0, named.1, linked.0, linked.1
        ));
    }
    if !lists(SUPPORTED_PLATFORMS, IPHONE_OS) {
        problems.push(format!("{SUPPORTED_PLATFORMS} does not list {IPHONE_OS}"));
    }
    if part == Part::App {
        app_problems(&keys, &mut problems);
    }
    if problems.is_empty() {
        return Ok(());
    }
    bail!(
        "App Store Connect would refuse {}'s Info.plist: {}",
        match part {
            Part::App => "this bundle",
            Part::Framework => "the native scripts framework",
        },
        problems.join("; ")
    )
}

/// What an app's plist answers for and a framework's does not: the devices
/// it runs on, and on an iPad its orientations.
fn app_problems(keys: &[(String, Value)], problems: &mut Vec<String>) {
    let value = |name: &str| keys.iter().find(|(key, _)| key == name).map(|(_, v)| v);
    let lists = |name: &str, item: &str| value(name).is_some_and(|v| v.lists(item));
    if !value(DEVICE_FAMILY).is_some_and(|v| matches!(v, Value::Items(f) if !f.is_empty())) {
        problems.push(format!(
            "{DEVICE_FAMILY} names no device: set [apple] devices (90102)"
        ));
    }
    if !lists(REQUIRED_CAPABILITIES, ARM64) {
        problems.push(format!(
            "{REQUIRED_CAPABILITIES} does not list {ARM64}, which a 64-bit binary needs (90502)"
        ));
    }
    let turns = |key: &str| EVERY_ORIENTATION.iter().all(|face| lists(key, face));
    let full_screen = value(REQUIRES_FULL_SCREEN) == Some(&Value::True);
    let ipad_turns = match value(IPAD_ORIENTATIONS) {
        Some(_) => turns(IPAD_ORIENTATIONS),
        None => turns(crate::apple::ORIENTATIONS),
    };
    let ipad = crate::apple::Device::Ipad.family().to_string();
    if lists(DEVICE_FAMILY, &ipad) && !full_screen && !ipad_turns {
        problems.push(format!(
            "an iPad build that multitasks turns to every orientation: list each in \
             {IPAD_ORIENTATIONS}, or set {REQUIRES_FULL_SCREEN} (90474)"
        ));
    }
}

/// The toolchain keys of the Xcode on this machine for `sdk` (`iphoneos` or
/// `iphonesimulator`), read as `scripts/package_runtime.sh` reads them: what
/// a bundle this exporter builds with that Xcode carries.
pub(crate) fn toolchain_here(sdk: &str) -> Result<Vec<(String, String)>> {
    let said = |command: &mut std::process::Command| -> Result<String> {
        let out = command.output().context("running the Xcode tools")?;
        anyhow::ensure!(
            out.status.success(),
            "{command:?} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let xcrun = |flag: &str| {
        said(std::process::Command::new(words::native::XCRUN).args(["--sdk", sdk, flag]))
    };
    let version = xcrun("--show-sdk-version")?;
    let xcode = said(std::process::Command::new(XCODEBUILD).arg("-version"))?;
    // "Xcode 27.1" then "Build version 27A9275".
    let word = |line: usize, at: usize| {
        xcode
            .lines()
            .nth(line)
            .and_then(|line| line.split_whitespace().nth(at))
            .map(str::to_string)
            .with_context(|| format!("{XCODEBUILD} -version said {xcode:?}"))
    };
    Ok(vec![
        (
            BUILD_MACHINE.into(),
            said(std::process::Command::new(SW_VERS).arg("-buildVersion"))?,
        ),
        (COMPILER_KEY.into(), COMPILER.into()),
        (PLATFORM_NAME.into(), sdk.into()),
        (PLATFORM_VERSION.into(), version.clone()),
        (SDK_BUILD.into(), xcrun("--show-sdk-build-version")?),
        (SDK_NAME.into(), format!("{sdk}{version}")),
        (XCODE.into(), dt_xcode(&word(0, 1)?)),
        (XCODE_BUILD.into(), word(1, 2)?),
    ])
}

/// Xcode's version as `DTXcode` writes it: two digits of major, one each
/// after, so 26.0.1 is 2601 and 9.4 is 0940.
pub(crate) fn dt_xcode(version: &str) -> String {
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0));
    let mut next = || parts.next().unwrap_or(0);
    format!("{:02}{}{}", next(), next(), next())
}

/// The entitlements a signature names the app by on `platform`, the
/// identifier and the team first, which
/// [`AppleConfig::signed_against`] copies from the provisioning profile.
pub(crate) const fn identity_keys(platform: Platform) -> [&'static str; 4] {
    match platform {
        Platform::Ios => [
            IOS_APPLICATION_IDENTIFIER,
            TEAM_IDENTIFIER,
            GET_TASK_ALLOW,
            BETA_REPORTS_ACTIVE,
        ],
        Platform::Macos => [
            APPLICATION_IDENTIFIER,
            TEAM_IDENTIFIER,
            BETA_REPORTS_ACTIVE,
            MAC_GET_TASK_ALLOW,
        ],
    }
}

/// Whether `key` is an application identifier, `<team>.<bundle id>`.
pub(crate) fn is_identifier(key: &str) -> bool {
    key == IOS_APPLICATION_IDENTIFIER || key == APPLICATION_IDENTIFIER
}

/// The identifier a profile grants as it names `bundle_id`: a wildcard,
/// `<team>.*` or `<team>.com.studio.*`, the bundle id after the team where it
/// matches; anything else as granted, for [`check_signature`] to weigh.
pub(crate) fn named_by(granted: &str, bundle_id: &str) -> String {
    match granted.split_once('.') {
        Some((team, pattern))
            if pattern
                .strip_suffix('*')
                .is_some_and(|stem| bundle_id.starts_with(stem)) =>
        {
            format!("{team}.{bundle_id}")
        }
        _ => granted.to_string(),
    }
}

/// Refuse a signature whose entitlements, `signed` as `codesign -d` reads
/// them back, do not name the app as its provisioning profile does: App
/// Store Connect refuses one that lacks them (90046) or claims other values
/// (90164). `apple` is the build signed against that profile.
pub(crate) fn check_signature(signed: &str, platform: Platform, apple: &AppleConfig) -> Result<()> {
    // `codesign -d` prints nothing for a signature that claims nothing.
    let claimed = if signed.trim().is_empty() {
        Vec::new()
    } else {
        plist::top_level(signed)?
    };
    let mut problems = Vec::new();
    for key in &identity_keys(platform)[..2] {
        if !apple.identity.iter().any(|(granted, _)| granted == key) {
            problems.push(format!("the provisioning profile grants no {key}"));
        }
    }
    for (key, granted) in &apple.identity {
        if is_identifier(key)
            && !granted
                .text()
                .is_some_and(|id| id.ends_with(&format!(".{}", apple.bundle_id)))
        {
            problems.push(format!(
                "the provisioning profile's {key} is {}, which does not name {}",
                granted.written().unwrap_or_default(),
                apple.bundle_id
            ));
        }
        match claimed.iter().find(|(name, _)| name == key) {
            None => problems.push(format!("the signature lacks {key} (90046)")),
            Some((_, value)) if value != granted => problems.push(format!(
                "the signature claims {key} as {}, and the profile grants {} (90164)",
                value.written().unwrap_or_default(),
                granted.written().unwrap_or_default()
            )),
            Some(_) => {}
        }
    }
    if problems.is_empty() {
        return Ok(());
    }
    bail!(
        "App Store Connect would refuse this signature: {}",
        problems.join("; ")
    )
}

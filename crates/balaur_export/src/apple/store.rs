//! What App Store Connect requires of an iOS bundle's `Info.plist` beyond
//! what `[apple]` declares: the keys Xcode writes from the SDK and the Xcode
//! it built with, which an export keeps from the template, and a check of
//! the app's own plist as an export wrote it, so a plist the store refuses
//! fails the export rather than the upload.

use anyhow::{Result, bail};

use crate::plist::{self, Value};

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
pub(crate) const EXECUTABLE: &str = "CFBundleExecutable";
pub(crate) const MINIMUM_OS: &str = "MinimumOSVersion";

/// The toolchain keys App Store Connect reads, each written by Xcode, and by
/// `scripts/package_runtime.sh` into a template, from the SDK and the Xcode
/// the build used.
const TOOLCHAIN_KEYS: [&str; 8] = [
    BUILD_MACHINE,
    "DTCompiler",
    PLATFORM_NAME,
    PLATFORM_VERSION,
    "DTSDKBuild",
    "DTSDKName",
    "DTXcode",
    "DTXcodeBuild",
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

/// Refuse a device bundle whose `Info.plist`, `text` as written with the
/// project's own keys in it, App Store Connect would refuse, naming every
/// key that is missing or wrong. A simulator's bundle is never uploaded.
pub(crate) fn check(text: &str, template: &IosTemplate) -> Result<()> {
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
            "it lacks {}, which the template's plist carries from the SDK and the Xcode it was \
             built with: build the template again with scripts/package_runtime.sh (90507)",
            missing.join(", ")
        ));
    }
    if let Some(name) = text_of(PLATFORM_NAME).filter(|name| *name != DEVICE_SDK) {
        problems.push(format!(
            "{PLATFORM_NAME} is {name}, and the binary is a device's: the template's plist was \
             written for another build"
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
    if problems.is_empty() {
        return Ok(());
    }
    bail!(
        "App Store Connect would refuse this bundle's Info.plist: {}",
        problems.join("; ")
    )
}

//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const EXE: &str = "exe";

    pub(crate) mod artifacts {
        pub(crate) const BUNDLE: &str = "bundle";
        pub(crate) const DYLIB: &str = "dylib";
        pub(crate) const JSON: &str = "json";
        pub(crate) const SO: &str = "so";
    }

    pub(crate) mod targets {
        pub(crate) const ANDROID: &str = "android";
        pub(crate) const ARM64_V8A: &str = "arm64-v8a";
        pub(crate) const IOS: &str = "ios";
        pub(crate) const LINUX_ARM64: &str = "linux-arm64";
        pub(crate) const LINUX_X64: &str = "linux-x64";
        pub(crate) const MACOS_UNIVERSAL: &str = "macos-universal";
        pub(crate) const WEB: &str = "web";
        pub(crate) const WINDOWS_ARM64: &str = "windows-arm64";
        pub(crate) const WINDOWS_X64: &str = "windows-x64";
        pub(crate) const X86_64: &str = "x86_64";
    }

    pub(crate) mod runtimes {
        pub(crate) const BARE: &str = "bare";
    }

    pub(crate) mod android {
        pub(crate) const LIB: &str = "lib";
    }

    pub(crate) mod bundle {
        pub(crate) const ASSETS: &str = "assets";
    }

    pub(crate) mod config {
        pub(crate) const EXPORT: &str = "export";
    }

    pub(crate) mod extensions {
        pub(crate) const DLL: &str = "dll";
    }

    pub(crate) mod native {
        pub(crate) const LINUX: &str = "linux";
        pub(crate) const MACOS: &str = "macos";
        pub(crate) const WINDOWS: &str = "windows";
        pub(crate) const XCRUN: &str = "xcrun";
    }

    pub(crate) mod sign {
        pub(crate) const SIGN: &str = "sign";
    }
}

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod keys {
    pub(crate) const NAME: &str = "name";
}

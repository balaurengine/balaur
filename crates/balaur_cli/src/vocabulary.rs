//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const SAME: &str = "same";
    pub(crate) const YES: &str = "yes";

    pub(crate) mod deps {
        pub(crate) const BAD_TREE: &str = "bad_tree";
        pub(crate) const BS: &str = "bs";
        pub(crate) const DEPS: &str = "deps";
        pub(crate) const PROJECT_NOT_FOUND: &str = "project_not_found";
        pub(crate) const TARGET: &str = "target";
        pub(crate) const TOML: &str = "toml";
        pub(crate) const UNKNOWN_REF: &str = "unknown_ref";
    }

    pub(crate) mod log_levels {
        pub(crate) const DEBUG: &str = "debug";
        pub(crate) const ERROR: &str = "error";
        pub(crate) const TRACE: &str = "trace";
        pub(crate) const WARN: &str = "warn";
    }

    pub(crate) mod lsp {
        pub(crate) const CONSTANT: &str = "constant";
        pub(crate) const EVENT: &str = "event";
        pub(crate) const EXIT: &str = "exit";
        pub(crate) const FIELD: &str = "field";
        pub(crate) const FUNCTION: &str = "function";
        pub(crate) const INITIALIZE: &str = "initialize";
        pub(crate) const METHOD: &str = "method";
        pub(crate) const MODULE: &str = "module";
        pub(crate) const PROPERTY: &str = "property";
        pub(crate) const SHUTDOWN: &str = "shutdown";
        pub(crate) const STRUCT: &str = "struct";
        pub(crate) const VARIABLE: &str = "variable";
    }

    pub(crate) mod targets {
        pub(crate) const ANDROID: &str = "android";
        pub(crate) const IOS: &str = "ios";
        pub(crate) const MACOS_UNIVERSAL: &str = "macos-universal";
        pub(crate) const PACK: &str = "pack";
        pub(crate) const WEB: &str = "web";
        pub(crate) const WINDOWS_ARM64: &str = "windows-arm64";
        pub(crate) const WINDOWS_X64: &str = "windows-x64";
    }

    pub(crate) mod export_api {
        pub(crate) const EXPORT: &str = "export";
    }

    pub(crate) mod export_shared {
        pub(crate) const FAILED: &str = "failed";
    }

    pub(crate) mod functions {
        pub(crate) const FILE: &str = "file";
        pub(crate) const OUTPUT: &str = "output";
        pub(crate) const PREVIEW: &str = "preview";
        pub(crate) const RUNNING_COUNT: &str = "running_count";
        pub(crate) const START: &str = "start";
        pub(crate) const TARGETS: &str = "targets";
    }

    pub(crate) mod general {
        pub(crate) const BALAUR: &str = "balaur";
    }

    pub(crate) mod project_api {
        pub(crate) const TEMPLATES: &str = "templates";
    }

    pub(crate) mod update {
        pub(crate) const DEV: &str = "dev";
    }

    pub(crate) mod web_export {
        pub(crate) const GAME: &str = "game";
    }

    pub(crate) mod web_store {
        pub(crate) const MODIFIED: &str = "modified";
    }
}

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod keys {
    pub(crate) const NOTE: &str = "note";
    pub(crate) const APPLICATION: &str = "application";
    pub(crate) const DONE: &str = "done";
    pub(crate) const ERROR: &str = "error";
    pub(crate) const ID: &str = "id";
    pub(crate) const INSTALLED: &str = "installed";
    pub(crate) const KIND: &str = "kind";
    pub(crate) const MESSAGE: &str = "message";
    pub(crate) const NAME: &str = "name";
    pub(crate) const OPENED: &str = "opened";
    pub(crate) const PATH: &str = "path";
    pub(crate) const PROJECT: &str = "project";
    /// When a release went out, in seconds since 1970; zero when unknown.
    pub(crate) const PUBLISHED: &str = "published";
    pub(crate) const SOURCE: &str = "source";
    pub(crate) const TAG: &str = "tag";
    pub(crate) const VERSION: &str = "version";
}

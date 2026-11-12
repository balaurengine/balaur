//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const CURRENT: &str = "current";
    pub(crate) const DURATION: &str = "duration";
    pub(crate) const EDITOR: &str = "editor";
    pub(crate) const FADE: &str = "fade";
    pub(crate) const FALSE: &str = "false";
    pub(crate) const MESSAGE: &str = "message";
    pub(crate) const NOW: &str = "now";
    pub(crate) const PROPERTIES: &str = "properties";
    pub(crate) const PROPS: &str = "props";
    pub(crate) const SCRIPT: &str = "script";
    pub(crate) const TARGET: &str = "target";
    pub(crate) const TEXT: &str = "text";
    pub(crate) const TRUE: &str = "true";
    pub(crate) const VISIBLE: &str = "visible";
    pub(crate) const KIND: &str = "kind";

    pub(crate) mod datatypes {
        pub(crate) const ASSET: &str = "asset";
        pub(crate) const BOOL: &str = "bool";
        pub(crate) const COLOR: &str = "color";
        pub(crate) const ENUM: &str = "enum";
        pub(crate) const FLAGS: &str = "flags";
        pub(crate) const FLOAT: &str = "float";
        pub(crate) const INT: &str = "int";
        pub(crate) const LIST: &str = "list";
        pub(crate) const MAP: &str = "map";
        pub(crate) const NODE: &str = "node";
        pub(crate) const RECORD: &str = "record";
        pub(crate) const STRING: &str = "string";
        pub(crate) const VARIANT: &str = "variant";
        pub(crate) const VEC2: &str = "vec2";
        pub(crate) const VEC3: &str = "vec3";
        pub(crate) const VEC4: &str = "vec4";
    }

    pub(crate) mod dependencies {
        pub(crate) const BRANCH: &str = "branch";
        pub(crate) const DEPS: &str = "deps";
        pub(crate) const PATH: &str = "path";
        pub(crate) const PROJECT: &str = "project";
        pub(crate) const TAG: &str = "tag";
    }

    pub(crate) mod file_types {
        pub(crate) const FLAC: &str = "flac";
        pub(crate) const FNT: &str = "fnt";
        pub(crate) const GLB: &str = "glb";
        pub(crate) const GLTF: &str = "gltf";
        pub(crate) const JPEG: &str = "jpeg";
        pub(crate) const JPG: &str = "jpg";
        pub(crate) const MP3: &str = "mp3";
        pub(crate) const OBJ: &str = "obj";
        pub(crate) const OGG: &str = "ogg";
        pub(crate) const PNG: &str = "png";
        pub(crate) const SVG: &str = "svg";
        pub(crate) const WAV: &str = "wav";
        pub(crate) const WEBP: &str = "webp";
    }

    pub(crate) mod locales {
        pub(crate) const BE: &str = "be";
        pub(crate) const BS: &str = "bs";
        pub(crate) const FR: &str = "fr";
        pub(crate) const HR: &str = "hr";
        pub(crate) const ID: &str = "id";
        pub(crate) const JA: &str = "ja";
        pub(crate) const KO: &str = "ko";
        pub(crate) const MO: &str = "mo";
        pub(crate) const NB: &str = "nb";
        pub(crate) const NN: &str = "nn";
        pub(crate) const PL: &str = "pl";
        pub(crate) const PT: &str = "pt";
        pub(crate) const RO: &str = "ro";
        pub(crate) const RU: &str = "ru";
        pub(crate) const SR: &str = "sr";
        pub(crate) const TH: &str = "th";
        pub(crate) const UK: &str = "uk";
        pub(crate) const VI: &str = "vi";
        pub(crate) const ZH: &str = "zh";
    }

    pub(crate) mod modules {
        pub(crate) const ASSETS: &str = "assets";
        pub(crate) const ENCODING: &str = "encoding";
        pub(crate) const ENGINE: &str = "engine";
        pub(crate) const FS: &str = "fs";
        pub(crate) const HASH: &str = "hash";
        pub(crate) const JSON: &str = "json";
        pub(crate) const LOG: &str = "log";
        pub(crate) const RANDOM: &str = "random";
        pub(crate) const REGEX: &str = "regex";
        pub(crate) const SAVE: &str = "save";
        pub(crate) const SCENE: &str = "scene";
        pub(crate) const SKELETON: &str = "skeleton";
        pub(crate) const STRINGS: &str = "strings";
        pub(crate) const TOML: &str = "toml";
    }

    pub(crate) mod platforms {
        pub(crate) const AARCH64: &str = "aarch64";
        pub(crate) const ANDROID: &str = "android";
        pub(crate) const ARM64: &str = "arm64";
        pub(crate) const IOS: &str = "ios";
        pub(crate) const MACOS: &str = "macos";
        pub(crate) const WASM32: &str = "wasm32";
        pub(crate) const WEB: &str = "web";
        pub(crate) const WINDOWS: &str = "windows";
        pub(crate) const X64: &str = "x64";
    }

    pub(crate) mod process_modes {
        pub(crate) const ALWAYS: &str = "always";
        pub(crate) const DISABLED: &str = "disabled";
        pub(crate) const INHERIT: &str = "inherit";
        pub(crate) const PAUSABLE: &str = "pausable";
    }

    pub(crate) mod window_modes {
        pub(crate) const DUPLICATE: &str = "duplicate";
        pub(crate) const EXCLUSIVE: &str = "exclusive";
        pub(crate) const FULLSCREEN: &str = "fullscreen";
        pub(crate) const LANDSCAPE: &str = "landscape";
        pub(crate) const MAXIMIZED: &str = "maximized";
        pub(crate) const PORTRAIT: &str = "portrait";
        pub(crate) const WINDOWED: &str = "windowed";
    }

    pub(crate) mod facts {
        pub(crate) const WASM: &str = "wasm";
    }

    pub(crate) mod functions {
        pub(crate) const CLEAR: &str = "clear";
        pub(crate) const DEVICE_ID: &str = "device_id";
        pub(crate) const DIRECTORY: &str = "directory";
        pub(crate) const ENCODE: &str = "encode";
        pub(crate) const EXISTS: &str = "exists";
        pub(crate) const INFO: &str = "info";
        pub(crate) const LOAD: &str = "load";
        pub(crate) const LOCALE: &str = "locale";
        pub(crate) const PARSE: &str = "parse";
        pub(crate) const PAUSED: &str = "paused";
        pub(crate) const QUIT: &str = "quit";
        pub(crate) const READ: &str = "read";
        pub(crate) const REMOVE: &str = "remove";
        pub(crate) const RENAME: &str = "rename";
        pub(crate) const ROOT: &str = "root";
        pub(crate) const TO_TOML: &str = "to_toml";
        pub(crate) const UNLOAD: &str = "unload";
        pub(crate) const VERSION: &str = "version";
        pub(crate) const WRITE: &str = "write";
    }

    pub(crate) mod replay_api {
        pub(crate) const SCRIPTS: &str = "scripts";
    }

    pub(crate) mod scene_toml {
        pub(crate) const NAME: &str = "name";
    }

    pub(crate) mod settings_api {
        pub(crate) const TAGS: &str = "tags";
    }
}

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod keys {
    pub(crate) const DURATION: &str = "duration";
    pub(crate) const IGNORE: &str = "ignore";
    pub(crate) const ROWS: &str = "rows";
    pub(crate) const SCRIPT: &str = "script";
    pub(crate) const SOURCE: &str = "source";
    pub(crate) const TAGS: &str = "tags";
    pub(crate) const TINT: &str = "tint";
    pub(crate) const VISIBLE: &str = "visible";
    pub(crate) const Z_AS_RELATIVE: &str = "z_as_relative";
    pub(crate) const Z_INDEX: &str = "z_index";
    pub(crate) const APPLICATION: &str = "application";
    pub(crate) const COMPONENT: &str = "component";
    pub(crate) const DATA: &str = "data";
    pub(crate) const DEFAULT: &str = "default";
    pub(crate) const DESCRIPTION: &str = "description";
    pub(crate) const FIELDS: &str = "fields";
    pub(crate) const FIRST: &str = "first";
    pub(crate) const HOST: &str = "host";
    pub(crate) const ID: &str = "id";
    pub(crate) const KIND: &str = "kind";
    pub(crate) const LAST: &str = "last";
    pub(crate) const NODES: &str = "nodes";
    pub(crate) const OF: &str = "of";
    pub(crate) const OPTIONS: &str = "options";
    pub(crate) const OVERRIDES: &str = "overrides";
    pub(crate) const PARENT: &str = "parent";
    pub(crate) const PATH: &str = "path";
    pub(crate) const PROPS: &str = "props";
    pub(crate) const TICK: &str = "tick";
    pub(crate) const TIME: &str = "time";
    pub(crate) const TYPE: &str = "type";
    pub(crate) const VALUE: &str = "value";
    pub(crate) const VARIANTS: &str = "variants";
}

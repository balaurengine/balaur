//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const ANDROID: &str = "android";
    pub(crate) const API: &str = "api";
    pub(crate) const EXPORTS: &str = "exports";

    pub(crate) mod doc_types {
        pub(crate) const ANY: &str = "any";
        pub(crate) const ASSET: &str = "asset";
        pub(crate) const BOOL: &str = "bool";
        pub(crate) const BYTES: &str = "bytes";
        pub(crate) const COLOR: &str = "color";
        pub(crate) const ENUM: &str = "enum";
        pub(crate) const FLOAT: &str = "float";
        pub(crate) const INT: &str = "int";
        pub(crate) const LIST: &str = "list";
        pub(crate) const MAP: &str = "map";
        pub(crate) const NODE: &str = "node";
        pub(crate) const NUMBER: &str = "number";
        pub(crate) const STR: &str = "str";
        pub(crate) const STRING: &str = "string";
        pub(crate) const TABLE: &str = "table";
        pub(crate) const VARIANT: &str = "variant";
        pub(crate) const VEC2: &str = "vec2";
        pub(crate) const VEC3: &str = "vec3";
    }

    pub(crate) mod rust_types {
        pub(crate) const F32: &str = "f32";
        pub(crate) const F64: &str = "f64";
        pub(crate) const I16: &str = "i16";
        pub(crate) const I32: &str = "i32";
        pub(crate) const I64: &str = "i64";
        pub(crate) const I8: &str = "i8";
        pub(crate) const ISIZE: &str = "isize";
        pub(crate) const U16: &str = "u16";
        pub(crate) const U32: &str = "u32";
        pub(crate) const U64: &str = "u64";
        pub(crate) const U8: &str = "u8";
        pub(crate) const USIZE: &str = "usize";
    }
}

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod keys {
    pub(crate) const ACTS_ON: &str = "acts_on";
    pub(crate) const CONSTANTS: &str = "constants";
    pub(crate) const DOC: &str = "doc";
    pub(crate) const EDITOR: &str = "editor";
    pub(crate) const FUNCTIONS: &str = "functions";
    pub(crate) const METHODS: &str = "methods";
    pub(crate) const NAME: &str = "name";
}

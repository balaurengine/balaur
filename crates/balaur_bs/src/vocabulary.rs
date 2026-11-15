//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) mod builtins {
        pub(crate) const COLOR: &str = "color";
        pub(crate) const VEC2: &str = "vec2";
        pub(crate) const VEC3: &str = "vec3";
    }

    pub(crate) mod host_calls {
        pub(crate) const COMPONENT_PROPERTY: &str = "component_property";
        pub(crate) const PATCH_COMPONENT: &str = "patch_component";
    }

    /// The modules the prelude writes, which a script calls by name.
    pub(crate) mod library {
        pub(crate) const AUDIO: &str = "audio";
        pub(crate) const ENCODING: &str = "encoding";
        pub(crate) const GEOMETRY_2D: &str = "geometry2d";
        pub(crate) const QUAT: &str = "quat";
        pub(crate) const RANDOM: &str = "random";
    }

    pub(crate) mod keywords {
        pub(crate) const ANY: &str = "any";
        pub(crate) const BOOL: &str = "bool";
        pub(crate) const FALSE: &str = "false";
        pub(crate) const FLOAT: &str = "float";
        pub(crate) const FN: &str = "fn";
        pub(crate) const IF: &str = "if";
        pub(crate) const INT: &str = "int";
        pub(crate) const MATCH: &str = "match";
        pub(crate) const NIL: &str = "nil";
        pub(crate) const SELF: &str = "self";
        pub(crate) const STR: &str = "str";
        pub(crate) const TRUE: &str = "true";
    }

    pub(crate) mod math {
        pub(crate) const ABS: &str = "abs";
        pub(crate) const ACOS: &str = "acos";
        pub(crate) const ASIN: &str = "asin";
        pub(crate) const ATAN: &str = "atan";
        pub(crate) const ATAN2: &str = "atan2";
        pub(crate) const CEIL: &str = "ceil";
        pub(crate) const COS: &str = "cos";
        pub(crate) const EXP: &str = "exp";
        pub(crate) const FLOOR: &str = "floor";
        pub(crate) const LN: &str = "ln";
        pub(crate) const LOG: &str = "log";
        pub(crate) const MAX: &str = "max";
        pub(crate) const MIN: &str = "min";
        pub(crate) const POW: &str = "pow";
        pub(crate) const SIN: &str = "sin";
        pub(crate) const SQRT: &str = "sqrt";
        pub(crate) const TAN: &str = "tan";
    }

    pub(crate) mod methods {
        pub(crate) const AS_BOOL: &str = "as_bool";
        pub(crate) const AS_FLOAT: &str = "as_float";
        pub(crate) const AS_INT: &str = "as_int";
        pub(crate) const AS_NODE: &str = "as_node";
        pub(crate) const AS_STR: &str = "as_str";
        pub(crate) const GET: &str = "get";
        pub(crate) const INSERT: &str = "insert";
        pub(crate) const LEN: &str = "len";
        pub(crate) const OR: &str = "or";
        pub(crate) const POP: &str = "pop";
        pub(crate) const PUSH: &str = "push";
        pub(crate) const REMOVE: &str = "remove";
        pub(crate) const SET: &str = "set";
        pub(crate) const UNWRAP: &str = "unwrap";
        pub(crate) const VALUES_IN_ORDER: &str = "values_in_order";
    }

    pub(crate) mod packages {
        pub(crate) const ADDONS: &str = "addons";
        pub(crate) const DEPS: &str = "deps";
    }

    pub(crate) mod convert {
        pub(crate) const VALUE: &str = "value";
    }

    pub(crate) mod general {
        pub(crate) const AWAIT: &str = "await";
        pub(crate) const ELSE: &str = "else";
        pub(crate) const FOR: &str = "for";
        pub(crate) const IN: &str = "in";
        pub(crate) const LET: &str = "let";
        pub(crate) const LOOP: &str = "loop";
        pub(crate) const RETURN: &str = "return";
        pub(crate) const WHILE: &str = "while";
    }

    pub(crate) mod parse {
        pub(crate) const ASYNC: &str = "async";
        pub(crate) const CLASS: &str = "class";
        pub(crate) const EVENT: &str = "event";
    }
}

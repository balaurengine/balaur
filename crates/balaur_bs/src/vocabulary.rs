//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) mod builtins {
        pub(crate) const COLOR: &str = "color";
        pub(crate) const VEC2: &str = "vec2";
        pub(crate) const VEC3: &str = "vec3";
    }

    pub(crate) mod droppable {
        pub(crate) const APPLE: &str = "apple";
        pub(crate) const BROWSER: &str = "browser";
        pub(crate) const CLOUD_READ: &str = "cloud_read";
        pub(crate) const CLOUD_WRITE: &str = "cloud_write";
        pub(crate) const CREDENTIAL_STATE: &str = "credential_state";
        pub(crate) const ENTITLEMENTS: &str = "entitlements";
        pub(crate) const EXPORT: &str = "export";
        pub(crate) const FINISH_PURCHASE: &str = "finish_purchase";
        pub(crate) const FORGE: &str = "forge";
        pub(crate) const GAMEND: &str = "gamend";
        pub(crate) const HTTP: &str = "http";
        pub(crate) const IDENTITY: &str = "identity";
        pub(crate) const IMPORT: &str = "import";
        pub(crate) const LISTEN: &str = "listen";
        pub(crate) const LOGIN: &str = "login";
        pub(crate) const NOTIFY: &str = "notify";
        pub(crate) const PICTURE: &str = "picture";
        pub(crate) const PLATFORM: &str = "platform";
        pub(crate) const PRODUCTS: &str = "products";
        pub(crate) const PROGRESS: &str = "progress";
        pub(crate) const PURCHASE: &str = "purchase";
        pub(crate) const REGISTER: &str = "register";
        pub(crate) const RELEASE: &str = "release";
        pub(crate) const REQUEST: &str = "request";
        pub(crate) const REQUEST_NOTIFICATIONS: &str = "request_notifications";
        pub(crate) const REST: &str = "rest";
        pub(crate) const RESTORE_PURCHASES: &str = "restore_purchases";
        pub(crate) const SCORES: &str = "scores";
        pub(crate) const SET_CAMERA: &str = "set_camera";
        pub(crate) const SET_CAMERA_2D: &str = "set_camera_2d";
        pub(crate) const SET_PRESENCE: &str = "set_presence";
        pub(crate) const SHOW_DASHBOARD: &str = "show_dashboard";
        pub(crate) const SIGN_IN: &str = "sign_in";
        pub(crate) const STOP_LISTENING: &str = "stop_listening";
        pub(crate) const SUBMIT_SCORE: &str = "submit_score";
        pub(crate) const UNLOCK: &str = "unlock";
    }

    pub(crate) mod host_calls {
        pub(crate) const COMPONENT_PROPERTY: &str = "component_property";
        pub(crate) const PATCH_COMPONENT: &str = "patch_component";
        pub(crate) const VISIBLE: &str = "visible";
        pub(crate) const WIDGET: &str = "widget";
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
        pub(crate) const AT: &str = "at";
        pub(crate) const EQUALS: &str = "equals";
        pub(crate) const GET: &str = "get";
        pub(crate) const INSERT: &str = "insert";
        pub(crate) const IS_NIL: &str = "is_nil";
        pub(crate) const IS_NONE: &str = "is_none";
        pub(crate) const IS_SOME: &str = "is_some";
        pub(crate) const LEN: &str = "len";
        pub(crate) const MAP: &str = "map";
        pub(crate) const OR: &str = "or";
        pub(crate) const POP: &str = "pop";
        pub(crate) const PUSH: &str = "push";
        pub(crate) const PUT: &str = "put";
        pub(crate) const REMOVE: &str = "remove";
        pub(crate) const SET: &str = "set";
        pub(crate) const UNWRAP: &str = "unwrap";
        pub(crate) const UNWRAP_OR: &str = "unwrap_or";
        pub(crate) const VALUES_IN_ORDER: &str = "values_in_order";
        pub(crate) const WITH: &str = "with";
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
    }
}

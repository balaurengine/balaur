//! The names a compiled module and its host agree on.

pub const HOST_MODULE: &str = "env";
pub const MEMORY: &str = "memory";
pub const ALLOC: &str = "__alloc";
pub const FREE: &str = "__free";
pub const LIVE: &str = "__live";
pub const DBG_RESET: &str = "__dbg_reset";
/// Followed by the type, `__dec$[str]`: releases one reference to a value of it.
pub const DEC: &str = "__dec$";

pub const START: &str = "$start";
pub const POLL: &str = "$poll";
pub const BATCH: &str = "$batch";

pub const FMT_I64: &str = "__fmt_i64";
pub const FMT_F64: &str = "__fmt_f64";
pub const AWAIT_I64: &str = "__await_i64";
pub const AWAIT_F64: &str = "__await_f64";
pub const AWAIT_I32: &str = "__await_i32";
pub const MATH_PREFIX: &str = "__math_";
pub const STR_PREFIX: &str = "__str_";
pub const ANY_PREFIX: &str = "__any_";
pub const DBG_PROBE: &str = "__dbg_probe";
/// Debug builds: the shadow stack's base and top, the stepping flag, and the
/// address of the breakpoint table, one byte per statement.
pub const DBG_BASE: &str = "__dbg_base";
pub const DBG_SP: &str = "__dbg_sp";
pub const DBG_FLAGS: &str = "__dbg_flags";
pub const DBG_BREAKS: &str = "__dbg_breaks";

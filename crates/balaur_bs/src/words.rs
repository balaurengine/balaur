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

pub const FMT_F64: &str = "__fmt_f64";
pub const AWAIT_I64: &str = "__await_i64";
pub const AWAIT_F64: &str = "__await_f64";
pub const AWAIT_I32: &str = "__await_i32";
pub const AWAIT_HEAP: &str = "__await_heap";
pub const MATH_PREFIX: &str = "__math_";
pub const STR_PREFIX: &str = "__str_";
pub const ANY_PREFIX: &str = "__any_";
pub const DBG_PROBE: &str = "__dbg_probe";
/// A typed value written to memory, read back by the host as an `any`.
pub const BOX: &str = "__box";
/// An `any` as a typed value, which the host writes to memory.
pub const UNBOX: &str = "__unbox";
/// Loop passes and calls left before a metered module traps.
pub const FUEL: &str = "__fuel";
/// A profiled module: the statements it has run, wrapping.
pub const STEPS: &str = "__steps";
/// A profiled module: where its table of calls and own statements per
/// function sits, set by the host; 0 keeps none.
pub const PROF: &str = "__prof";
/// A direct module's entry point for a call of `n` slots: `__bs_call{n}`.
pub const DIRECT_CALL: &str = "__bs_call";
/// A direct module's way to the second and later results of a call.
pub const DIRECT_RESULT: &str = "__bs_result";
/// The global a direct module passes as its instance's id.
pub const HOST_CTX: &str = "__host_ctx";
/// Debug builds: the shadow stack's base and top, the stepping flag, and the
/// address of the breakpoint table, one byte per statement.
pub const DBG_BASE: &str = "__dbg_base";
pub const DBG_SP: &str = "__dbg_sp";
pub const DBG_FLAGS: &str = "__dbg_flags";
pub const DBG_BREAKS: &str = "__dbg_breaks";

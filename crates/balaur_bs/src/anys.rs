// An `any` holds a value whose kind is known only when the script runs: what
// the engine hands over for an http reply, a gamend record or an event. It is
// a refcounted cell in the module's memory, and nil is the pointer 0. Its
// methods are the prelude's (`prelude/any.bs`).

/// rc, tag, then 8 bytes of payload: the value, or one or two pointers.
pub(crate) const CELL: i32 = 16;

pub(crate) const BOOL: i32 = 1;
pub(crate) const INT: i32 = 2;
pub(crate) const FLOAT: i32 = 3;
pub(crate) const STR: i32 = 4;
/// The payload points at an `[any]`.
pub(crate) const LIST: i32 = 5;
/// The payload points at a `[str]` of keys, then an `[any]` of values.
pub(crate) const MAP: i32 = 6;
pub(crate) const NODE: i32 = 7;

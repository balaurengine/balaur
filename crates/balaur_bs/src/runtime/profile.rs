// What a profiled module counted: the statements it has run, and for each
// function the calls and its own statements, in a table the host gives it.

use super::{RunError, Runtime};
use crate::words;

/// Bytes a function's row takes: its calls, then its own statements.
const ROW: usize = 16;

impl Runtime {
    /// Give a profiled module its table, every row at nothing. A module
    /// built without counters has none to give.
    pub(super) fn arm_profile(&mut self) -> Result<(), RunError> {
        if !self.meta.profiled {
            return Ok(());
        }
        let bytes = self.meta.func_names.len() * ROW;
        let at = self.alloc(u32::try_from(bytes.max(ROW)).unwrap_or(u32::MAX))?;
        self.wasm.set_global(words::PROF, at as i32);
        Ok(())
    }

    /// The statements the module has run, wrapping: the difference across a
    /// call is what the call cost. Nil for a module built without counters.
    pub fn steps(&mut self) -> Option<i32> {
        if !self.meta.profiled {
            return None;
        }
        self.wasm.global(words::STEPS)
    }

    /// Each function that ran, as `(name, calls, own statements)`.
    pub fn function_costs(&mut self) -> Vec<(String, u64, u64)> {
        let Some(at) = self.table() else {
            return Vec::new();
        };
        let bytes = self.wasm.read(at, self.meta.func_names.len() * ROW);
        let (rows, _) = bytes.as_chunks::<ROW>();
        self.meta
            .func_names
            .iter()
            .zip(rows)
            .filter_map(|(name, row)| {
                let calls = u64::from_le_bytes(row[..8].try_into().ok()?);
                let own = i64::from_le_bytes(row[8..].try_into().ok()?);
                (calls > 0).then(|| (name.clone(), calls, own.max(0).cast_unsigned()))
            })
            .collect()
    }

    /// Every row back to nothing.
    pub fn clear_profile(&mut self) {
        if let Some(at) = self.table() {
            self.wasm
                .write(at, &vec![0; self.meta.func_names.len() * ROW]);
        }
    }

    fn table(&mut self) -> Option<usize> {
        if !self.meta.profiled {
            return None;
        }
        let at = self.wasm.global(words::PROF)?;
        (at != 0).then_some(at as u32 as usize)
    }
}

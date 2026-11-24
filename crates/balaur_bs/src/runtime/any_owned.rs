//! An `any` handed to a script, written with the module's heap lent once.
//!
//! A parsed document is hundreds of thousands of blocks: a cell a value, a
//! block a string, two a list and four a map. Taking each through the
//! engine (its globals read and set, its memory borrowed again, the bytes
//! made in a buffer of their own and copied) was most of what a word pack
//! cost to hand over. Here the value is walked twice: once for the size of
//! every block it needs, in the order the host would ask for them, and
//! once to write each where it was given, into the memory as one slice.
//! The blocks are taken in that order by [`heap::take`], so the heap ends
//! exactly as a block-at-a-time walk would leave it; one the bump pointer
//! cannot meet without growing the memory is the module's to give, between
//! two loans.

use super::engine::ImportCtx;
use super::heap::Lent;
use super::values::Value;
use crate::anys::{BOOL, CELL, FLOAT, INT, LIST, MAP, NODE, STR};
use smol_str::SmolStr;

/// A slot of a list of `any`: the cell's address as the script's `i32`.
const SLOT: usize = 8;
/// A list's or a map's half's header.
const HEADER: usize = 16;

/// The cell holding `v` (0 for nil), every block it needs taken and written
/// with the heap lent; `None` where this engine lends no heap, and nothing
/// was taken.
pub(crate) fn any_owned(ctx: &mut dyn ImportCtx, v: &Value) -> Result<Option<u32>, String> {
    let mut sizes = Vec::new();
    plan(v, &mut sizes);
    if sizes.is_empty() {
        return Ok(Some(0));
    }
    let mut at = Vec::with_capacity(sizes.len());
    while at.len() < sizes.len() {
        let lent = ctx.lend_heap(&mut |heap: &mut Lent<'_>| {
            while let Some(size) = sizes.get(at.len()) {
                match heap.take(*size) {
                    Some(block) => at.push(block),
                    None => break,
                }
            }
        });
        if !lent {
            // Only on the first loan: an engine lends always or never.
            return Ok(None);
        }
        if let Some(size) = sizes.get(at.len()) {
            // The memory must grow, which only the module does.
            at.push(u32::try_from(ctx.alloc(*size)?).map_err(|e| e.to_string())?);
        }
    }
    let mut cell = Ok(0);
    ctx.lend_heap(&mut |heap: &mut Lent<'_>| {
        let mut blocks = Blocks {
            memory: &mut *heap.memory,
            at: at.iter(),
        };
        cell = blocks.write(v);
    });
    cell.map(Some)
}

/// The size of each block `v` needs, in the order [`Blocks::write`] takes
/// them: what a value holds before the block holding it.
fn plan(v: &Value, sizes: &mut Vec<usize>) {
    match v {
        Value::Void | Value::Vector(_) => return,
        Value::Bool(_) | Value::Int(_) | Value::Node(_) | Value::Float(_) => {}
        Value::Str(s) => sizes.push(8 + s.len()),
        Value::Array(items) => {
            if !items.is_empty() {
                sizes.push(items.len() * SLOT);
            }
            for item in items {
                plan(item, sizes);
            }
            sizes.push(HEADER);
        }
        Value::Map(entries) | Value::Struct(_, entries) => {
            if !entries.is_empty() {
                sizes.push(entries.len() * SLOT);
            }
            for (key, _) in entries {
                sizes.push(8 + key.len());
            }
            sizes.push(HEADER);
            if !entries.is_empty() {
                sizes.push(entries.len() * SLOT);
            }
            for (_, value) in entries {
                plan(value, sizes);
            }
            sizes.push(HEADER);
        }
    }
    sizes.push(CELL as usize);
}

/// The lent memory, and the blocks taken for a value in [`plan`]'s order.
struct Blocks<'a, 'b> {
    memory: &'a mut [u8],
    at: std::slice::Iter<'b, u32>,
}

impl Blocks<'_, '_> {
    fn next(&mut self) -> Result<u32, String> {
        self.at
            .next()
            .copied()
            .ok_or_else(|| "a value needed more blocks than it was given".to_string())
    }

    // The `len` bytes at `at`, inside the memory.
    fn place(&mut self, at: u32, len: usize) -> Result<&mut [u8], String> {
        let at = at as usize;
        self.memory
            .get_mut(at..at + len)
            .ok_or_else(|| format!("a block at {at} is past the module's memory"))
    }

    // A block's words at `at`: its count of one, then `words`, then `rest`
    // (a string's text, a cell's payload), written where they go, with no
    // buffer of their own.
    fn put(&mut self, at: u32, words: &[u32], rest: &[u8]) -> Result<(), String> {
        let place = self.place(at, 4 + 4 * words.len() + rest.len())?;
        place[..4].copy_from_slice(&1u32.to_le_bytes());
        for (i, word) in words.iter().enumerate() {
            place[4 + 4 * i..8 + 4 * i].copy_from_slice(&word.to_le_bytes());
        }
        place[4 + 4 * words.len()..].copy_from_slice(rest);
        Ok(())
    }

    // The `i`th slot of a list's items at `data`: the address as the script
    // reads an `i32` widened to its 8-byte slot.
    fn slot(&mut self, data: u32, i: usize, p: u32) -> Result<(), String> {
        let at = data + u32::try_from(i * SLOT).map_err(|e| e.to_string())?;
        self.place(at, SLOT)?
            .copy_from_slice(&i64::from(p.cast_signed()).to_le_bytes());
        Ok(())
    }

    fn string(&mut self, s: &str) -> Result<u32, String> {
        let p = self.next()?;
        let len = u32::try_from(s.len()).map_err(|e| e.to_string())?;
        self.put(p, &[len], s.as_bytes())?;
        Ok(p)
    }

    fn cell(&mut self, tag: i32, payload: u64) -> Result<u32, String> {
        let p = self.next()?;
        self.put(p, &[tag.cast_unsigned()], &payload.to_le_bytes())?;
        Ok(p)
    }

    // A list's items, each written by `item`, under a header of its own.
    fn list<T>(
        &mut self,
        items: &[T],
        mut item: impl FnMut(&mut Self, &T) -> Result<u32, String>,
    ) -> Result<u32, String> {
        let data = if items.is_empty() { 0 } else { self.next()? };
        for (i, it) in items.iter().enumerate() {
            let p = item(self, it)?;
            self.slot(data, i, p)?;
        }
        let h = self.next()?;
        let len = u32::try_from(items.len()).map_err(|e| e.to_string())?;
        self.put(h, &[len, len, data], &[])?;
        Ok(h)
    }

    /// The cell holding `v`, 0 for nil.
    fn write(&mut self, v: &Value) -> Result<u32, String> {
        Ok(match v {
            Value::Void | Value::Vector(_) => 0,
            Value::Bool(b) => self.cell(BOOL, u64::from(*b))?,
            Value::Int(x) => self.cell(INT, x.cast_unsigned())?,
            Value::Node(x) => self.cell(NODE, x.cast_unsigned())?,
            Value::Float(x) => self.cell(FLOAT, x.to_bits())?,
            Value::Str(s) => {
                let p = self.string(s)?;
                self.cell(STR, u64::from(p))?
            }
            Value::Array(items) => {
                let h = self.list(items, Self::write)?;
                self.cell(LIST, u64::from(h))?
            }
            Value::Map(entries) | Value::Struct(_, entries) => {
                let keys = self.list(entries, |blocks, entry: &(SmolStr, Value)| {
                    blocks.string(&entry.0)
                })?;
                let values = self.list(entries, |blocks, entry: &(SmolStr, Value)| {
                    blocks.write(&entry.1)
                })?;
                self.cell(MAP, u64::from(keys) | (u64::from(values) << 32))?
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::heap;
    use super::super::values::{array_header, cell_bytes, str_bytes};
    use super::*;

    #[test]
    fn a_plan_asks_for_every_block_a_walk_writes() {
        let v = Value::Map(vec![
            ("word".into(), Value::Str("café".into())),
            (
                "facts".into(),
                Value::Map(vec![("gender".into(), Value::Str("m".into()))]),
            ),
            (
                "forms".into(),
                Value::Array(vec![Value::Int(1), Value::Void]),
            ),
            ("empty".into(), Value::Array(vec![])),
        ]);
        let mut sizes = Vec::new();
        plan(&v, &mut sizes);
        let mut memory = vec![0u8; 1 << 16];
        let mut bump = 1024u32;
        let at: Vec<u32> = sizes
            .iter()
            .map(|size| {
                let (at, after) = heap::take(&mut memory, bump, *size).unwrap();
                bump = after;
                at
            })
            .collect();
        let mut blocks = Blocks {
            memory: &mut memory,
            at: at.iter(),
        };
        let cell = blocks.write(&v).unwrap();
        assert_eq!(blocks.at.len(), 0, "every block taken was written");
        assert_eq!(cell, *at.last().unwrap(), "the cell is the last block");
        let read = |at: u32, len: usize| memory[at as usize..at as usize + len].to_vec();
        let word = |at: u32| u32::from_le_bytes(read(at, 4).try_into().unwrap());
        let keys = word(cell + 8);
        let values = word(cell + 12);
        assert_eq!(
            read(cell, 8),
            cell_bytes(MAP, u64::from(keys) | (u64::from(values) << 32))[..8].to_vec(),
            "a map's cell, as the host's helpers write one"
        );
        assert_eq!(
            read(keys, 4),
            array_header(4, word(keys + 12))[..4].to_vec()
        );
        assert_eq!(word(keys + 4), 4, "four keys");
        let first_key = word(word(keys + 12));
        assert_eq!(read(first_key, 12), str_bytes("word"), "the first key");
        let first_value = word(word(values + 12));
        let text = word(first_value + 8);
        assert_eq!(read(text, 13), str_bytes("café"), "the word, its bytes");
        assert_eq!(word(first_value + 4), STR.cast_unsigned());
    }
}

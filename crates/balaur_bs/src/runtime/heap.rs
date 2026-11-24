//! The module's allocator, run from the host.
//!
//! Every string, list and `any` cell a host function hands a script is a
//! block the module's own `__alloc` gives (`codegen::rt`), and calling into
//! the module for each one was most of what a large answer cost: a parsed
//! document is a block a value. [`take`] is the same steps, taken on the
//! module's memory and its allocator's globals from here, so the heap ends
//! exactly as the module's call would have left it, block for block: the
//! same size class, the same free list popped or the same bump. What only
//! the module does, growing its memory and trapping on a request past
//! 1 GiB, it is still asked to do.

use crate::codegen::rt::FREE_LISTS;

/// The largest request the module's allocator serves; past it, it traps.
const LARGEST: usize = 1 << 30;
/// A block's head: its size class, then padding, before what it holds.
const HEAD: u32 = 8;
/// The smallest block, head included: size class 0.
const SMALLEST: u32 = 16;

/// The block the module's `__alloc` would give for `size` bytes, taken: its
/// payload's address and where the bump pointer stands after it. `None`
/// where only the module can serve the request: one past 1 GiB, or one the
/// bump pointer cannot meet without growing the memory. A block taken here
/// counts towards the module's `__live` as one of its own does; the caller
/// adds it, with the new bump.
pub(crate) fn take(memory: &mut [u8], bump: u32, size: usize) -> Option<(u32, u32)> {
    if size > LARGEST {
        return None;
    }
    let need = u32::try_from(size).ok()? + HEAD;
    let mut block = SMALLEST;
    let mut class = 0u32;
    while block < need {
        block <<= 1;
        class += 1;
    }
    let list = FREE_LISTS as usize + class as usize * 4;
    let head = word(memory, list)?;
    let (at, bump) = if head == 0 {
        let end = bump.checked_add(block)?;
        if end as usize > memory.len() {
            return None;
        }
        (bump, end)
    } else {
        let next = word(memory, head as usize)?;
        set_word(memory, list, next)?;
        (head, bump)
    };
    set_word(memory, at as usize, class)?;
    Some((at + HEAD, bump))
}

/// The module's memory and its allocator's bump pointer and live count, lent
/// together to a host taking and writing many blocks at once
/// (`ImportCtx::lend_heap`): each block taken as [`take`] takes it, and the
/// bump and the count handed back when the loan ends.
pub(crate) struct Lent<'a> {
    pub(crate) memory: &'a mut [u8],
    pub(crate) bump: u32,
    pub(crate) live: i32,
}

impl Lent<'_> {
    /// A block of `size` bytes, counted as live; `None` where only the module
    /// can serve it, as [`take`].
    pub(crate) fn take(&mut self, size: usize) -> Option<u32> {
        let (at, bump) = take(self.memory, self.bump, size)?;
        self.bump = bump;
        self.live = self.live.wrapping_add(1);
        Some(at)
    }
}

fn word(memory: &[u8], at: usize) -> Option<u32> {
    let bytes = memory.get(at..at + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn set_word(memory: &mut [u8], at: usize, value: u32) -> Option<()> {
    memory
        .get_mut(at..at + 4)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The module's steps, as `codegen::rt::alloc` spells them, for a memory
    // that never grows: what `take` must agree with block for block.
    fn guest(memory: &mut [u8], bump: &mut u32, size: u32) -> u32 {
        let need = size + HEAD;
        let (mut block, mut class) = (SMALLEST, 0u32);
        while block < need {
            block <<= 1;
            class += 1;
        }
        let list = FREE_LISTS as usize + class as usize * 4;
        let head = word(memory, list).unwrap();
        let at = if head == 0 {
            let at = *bump;
            *bump += block;
            at
        } else {
            set_word(memory, list, word(memory, head as usize).unwrap()).unwrap();
            head
        };
        set_word(memory, at as usize, class).unwrap();
        at + HEAD
    }

    // `codegen::rt::free`.
    fn free(memory: &mut [u8], payload: u32) {
        let at = payload - HEAD;
        let list = FREE_LISTS as usize + word(memory, at as usize).unwrap() as usize * 4;
        set_word(memory, at as usize, word(memory, list).unwrap()).unwrap();
        set_word(memory, list, at).unwrap();
    }

    #[test]
    fn a_block_taken_here_is_the_one_the_module_would_give() {
        let mut ours = vec![0u8; 1 << 16];
        let mut theirs = ours.clone();
        let (mut our_bump, mut their_bump) = (1024u32, 1024u32);
        let mut held = Vec::new();
        for (step, size) in [0usize, 1, 8, 9, 24, 100, 7, 4000, 16, 300]
            .iter()
            .enumerate()
        {
            let (at, bump) = take(&mut ours, our_bump, *size).unwrap();
            our_bump = bump;
            assert_eq!(
                at,
                guest(&mut theirs, &mut their_bump, *size as u32),
                "step {step}"
            );
            held.push(at);
            // Every other block back to its free list, on both sides.
            if step % 2 == 1 {
                let back = held.remove(0);
                free(&mut ours, back);
                free(&mut theirs, back);
            }
        }
        assert_eq!(our_bump, their_bump);
        assert_eq!(ours, theirs, "the two heaps parted");
    }

    #[test]
    fn what_only_the_module_can_serve_is_left_to_it() {
        let mut memory = vec![0u8; 4096];
        assert_eq!(
            take(&mut memory, 4096 - 16, 9),
            None,
            "the memory would have to grow"
        );
        assert!(
            take(&mut memory, 4096 - 16, 8).is_some(),
            "a block that fits exactly"
        );
        assert_eq!(
            take(&mut memory, 1024, LARGEST + 1),
            None,
            "a request the module traps on"
        );
    }
}

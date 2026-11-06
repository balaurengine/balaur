// The release functions generated for a type the module touches.

use super::rt::{self, mem};
use super::{FuncBody, Gen, OBJ_HEADER};
use crate::ast::Ty;
use wasm_encoder::{BlockType, Instruction as I, ValType};

impl Gen {
    // dec for an array: release each element's heap fields, then its storage.
    pub(super) fn dec_array(&self, et: &Ty) -> FuncBody {
        let esz = self.elem_size(et) as i32;
        let heap: Vec<(u64, u32)> = self
            .slots(et)
            .into_iter()
            .enumerate()
            .filter_map(|(j, (_, h))| h.map(|h| (8 * j as u64, self.dec_fn(&h))))
            .collect();
        let mut drop = vec![];
        if !heap.is_empty() {
            drop.extend([
                I::LocalGet(0),
                I::I32Load(mem(12, 2)),
                I::LocalSet(3),
                I::Block(BlockType::Empty),
                I::Loop(BlockType::Empty),
            ]);
            drop.extend([
                I::LocalGet(2),
                I::LocalGet(0),
                I::I32Load(mem(4, 2)),
                I::I32GeU,
                I::BrIf(1),
            ]);
            for (off, f) in heap {
                drop.extend([
                    I::LocalGet(3),
                    I::LocalGet(2),
                    I::I32Const(esz),
                    I::I32Mul,
                    I::I32Add,
                    I::I32Load(mem(off, 2)),
                    I::Call(f),
                ]);
            }
            drop.extend([
                I::LocalGet(2),
                I::I32Const(1),
                I::I32Add,
                I::LocalSet(2),
                I::Br(0),
                I::End,
                I::End,
            ]);
        }
        drop.extend([
            I::LocalGet(0),
            I::I32Load(mem(12, 2)),
            I::If(BlockType::Empty),
            I::LocalGet(0),
            I::I32Load(mem(12, 2)),
            I::Call(self.h.free),
            I::End,
        ]);
        FuncBody {
            locals: vec![ValType::I32; 3],
            code: rt::dec_body(self.h, &drop),
        }
    }

    // dec for a class object: release each heap field, then the object.
    pub(super) fn dec_class(&self, s: &str) -> FuncBody {
        let mut drop = vec![];
        for (j, (_, heap)) in self.object_slots(s).into_iter().enumerate() {
            if let Some(h) = heap {
                drop.extend([
                    I::LocalGet(0),
                    I::I32Load(mem(OBJ_HEADER + 8 * j as u64, 2)),
                    I::Call(self.dec_fn(&h)),
                ]);
            }
        }
        FuncBody {
            locals: vec![ValType::I32],
            code: rt::dec_body(self.h, &drop),
        }
    }

    // dec for an `any`: release what its cell points at, by its tag.
    pub(super) fn dec_any(&self) -> FuncBody {
        use crate::anys::{LIST, MAP, STR};
        let anys = self.dec_fn(&Ty::Array(Box::new(Ty::Any)));
        let strs = self.dec_fn(&Ty::Array(Box::new(Ty::Str)));
        let when = |tag: i32, then: &[I<'static>]| {
            let mut c = vec![
                I::LocalGet(2),
                I::I32Const(tag),
                I::I32Eq,
                I::If(BlockType::Empty),
            ];
            c.extend(then.iter().cloned());
            c.push(I::End);
            c
        };
        let at = |off: u64, f: u32| [I::LocalGet(0), I::I32Load(mem(off, 2)), I::Call(f)];
        let mut drop = vec![I::LocalGet(0), I::I32Load(mem(4, 2)), I::LocalSet(2)];
        drop.extend(when(STR, &at(8, self.h.dec_str)));
        drop.extend(when(LIST, &at(8, anys)));
        let mut map = at(8, strs).to_vec();
        map.extend(at(12, anys));
        drop.extend(when(MAP, &map));
        FuncBody {
            locals: vec![ValType::I32; 3],
            code: rt::dec_body(self.h, &drop),
        }
    }
}

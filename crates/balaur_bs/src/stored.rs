// A build as bytes, so a host can keep each unit's between runs and skip
// the compiler when nothing it was built from changed. Lengths and counts are
// little-endian u32s, a type a tag byte and its parts; `linked` is not kept.

use crate::{
    Compiled, ExternInfo, FnInfo, HostSignature, Import, Link, Literal, Meta, Slot, Ty, Unit,
};

/// Changes whenever the layout below does. A build kept by another compiler,
/// told by the hash of its source, is read as none too.
const FORMAT: u32 = 1;

impl Compiled {
    /// This build as bytes: [`Compiled::from_bytes`] reads it back. A debug
    /// build's probes are not kept, nor `linked`.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer(Vec::with_capacity(self.wasm.len() + 4096));
        w.u32(FORMAT);
        w.str(env!("BALAUR_BS_SOURCE"));
        w.bytes(&self.wasm);
        w.str(&self.key);
        let m = &self.meta;
        w.list(&m.structs, |w, (n, fields)| {
            w.str(n);
            w.list(fields, |w, (f, t)| {
                w.str(f);
                w.ty(t);
            });
        });
        w.list(&m.classes, |w, c| w.str(c));
        w.list(&m.fns, |w, f| {
            w.str(&f.key);
            w.str(f.owner.as_deref().unwrap_or_default());
            w.list(&f.params, Writer::ty);
            w.ty(&f.ret);
            w.bool(f.is_async);
            w.bool(f.public);
            w.u32(f.frame_size);
        });
        w.list(&m.externs, |w, x| {
            w.str(&x.name);
            w.list(&x.params, Writer::ty);
            w.ty(&x.ret);
            w.bool(x.is_async);
        });
        w.list(&m.func_names, |w, n| w.str(n));
        w.bool(m.metered);
        w.bool(m.profiled);
        w.bool(m.direct);
        w.list(&m.uses, |w, u| w.str(u));
        w.list(&m.imports, |w, i| {
            w.str(&i.name);
            w.slots(&i.params);
            w.slots(&i.results);
        });
        w.list(&m.host_calls, |w, h| {
            w.str(&h.key);
            w.list(&h.params, Writer::ty);
            w.ty(&h.ret);
            w.bool(h.is_async);
        });
        w.list(&m.defaults, |w, (n, fields)| {
            w.str(n);
            w.list(fields, |w, (f, public, lit)| {
                w.str(f);
                w.bool(*public);
                w.literal(lit);
            });
        });
        w.bool(m.unit.is_some());
        if let Some(u) = &m.unit {
            w.u32(u.data_len);
            w.u32(u.refs);
            w.list(&u.links, |w, l| {
                w.str(&l.file);
                w.str(&l.export);
                w.slots(&l.params);
                w.slots(&l.results);
            });
        }
        w.0
    }

    /// A build [`Compiled::to_bytes`] wrote, or `None` for bytes it did not
    /// write, or that another build of the compiler did.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Compiled> {
        let mut r = Reader(bytes);
        if r.u32()? != FORMAT || r.str()? != env!("BALAUR_BS_SOURCE") {
            return None;
        }
        let wasm = r.bytes()?.to_vec();
        let key = r.str()?;
        let structs = r.list(|r| Some((r.str()?, r.list(|r| Some((r.str()?, r.ty()?)))?)))?;
        let classes = r.list(Reader::str)?;
        let fns = r.list(|r| {
            Some(FnInfo {
                key: r.str()?,
                // "" for none: no struct is named that.
                owner: Some(r.str()?).filter(|o| !o.is_empty()),
                params: r.list(Reader::ty)?,
                ret: r.ty()?,
                is_async: r.bool()?,
                public: r.bool()?,
                frame_size: r.u32()?,
            })
        })?;
        let externs = r.list(|r| {
            Some(ExternInfo {
                name: r.str()?,
                params: r.list(Reader::ty)?,
                ret: r.ty()?,
                is_async: r.bool()?,
            })
        })?;
        let func_names = r.list(Reader::str)?;
        let (metered, profiled, direct) = (r.bool()?, r.bool()?, r.bool()?);
        let uses = r.list(Reader::str)?;
        let imports = r.list(|r| {
            Some(Import {
                name: r.str()?,
                params: r.slots()?,
                results: r.slots()?,
            })
        })?;
        let host_calls = r.list(|r| {
            Some(HostSignature {
                key: r.str()?,
                params: r.list(Reader::ty)?,
                ret: r.ty()?,
                is_async: r.bool()?,
            })
        })?;
        let defaults = r.list(|r| {
            Some((
                r.str()?,
                r.list(|r| Some((r.str()?, r.bool()?, r.literal()?)))?,
            ))
        })?;
        let unit = if r.bool()? {
            Some(Unit {
                data_len: r.u32()?,
                refs: r.u32()?,
                links: r.list(|r| {
                    Some(Link {
                        file: r.str()?,
                        export: r.str()?,
                        params: r.slots()?,
                        results: r.slots()?,
                    })
                })?,
            })
        } else {
            None
        };
        let (by_key, by_owner) = crate::codegen::lookups(&fns);
        let meta = Meta {
            structs,
            classes,
            fns,
            by_key,
            by_owner,
            externs,
            probes: vec![],
            func_names,
            debug: false,
            metered,
            profiled,
            direct,
            uses,
            imports,
            host_calls,
            defaults,
            unit,
        };
        r.0.is_empty().then_some(Compiled {
            wasm,
            meta,
            key,
            linked: vec![],
        })
    }
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(u32::try_from(n).unwrap_or(u32::MAX));
    }
    fn bool(&mut self, b: bool) {
        self.0.push(u8::from(b));
    }
    fn bytes(&mut self, b: &[u8]) {
        self.len(b.len());
        self.0.extend_from_slice(b);
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn list<T>(&mut self, items: &[T], mut each: impl FnMut(&mut Self, &T)) {
        self.len(items.len());
        for item in items {
            each(self, item);
        }
    }
    fn slots(&mut self, slots: &[Slot]) {
        self.list(slots, |w, s| {
            w.0.push(match s {
                Slot::I32 => 0,
                Slot::I64 => 1,
                Slot::F64 => 2,
            });
        });
    }
    fn ty(&mut self, t: &Ty) {
        match t {
            Ty::Int => self.0.push(0),
            Ty::Float => self.0.push(1),
            Ty::Bool => self.0.push(2),
            Ty::Str => self.0.push(3),
            Ty::Node => self.0.push(4),
            Ty::Void => self.0.push(5),
            Ty::Any => self.0.push(6),
            Ty::Array(e) => {
                self.0.push(7);
                self.ty(e);
            }
            Ty::Optional(e) => {
                self.0.push(8);
                self.ty(e);
            }
            Ty::Struct(s) => {
                self.0.push(9);
                self.str(s);
            }
            Ty::Fn(params, ret, refs) => {
                self.0.push(10);
                self.list(params, Writer::ty);
                self.ty(ret);
                self.list(refs, |w, r| w.bool(*r));
            }
        }
    }
    fn literal(&mut self, l: &Literal) {
        match l {
            Literal::Int(x) => {
                self.0.push(0);
                self.0.extend_from_slice(&x.to_le_bytes());
            }
            Literal::Float(x) => {
                self.0.push(1);
                self.0.extend_from_slice(&x.to_bits().to_le_bytes());
            }
            Literal::Bool(b) => {
                self.0.push(2);
                self.bool(*b);
            }
            Literal::Str(s) => {
                self.0.push(3);
                self.str(s);
            }
        }
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
    fn byte(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn bool(&mut self) -> Option<bool> {
        Some(self.byte()? != 0)
    }
    fn bytes(&mut self) -> Option<&[u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    fn str(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?.to_vec()).ok()
    }
    fn list<T>(&mut self, mut each: impl FnMut(&mut Self) -> Option<T>) -> Option<Vec<T>> {
        let n = self.u32()? as usize;
        // A count past what is left is no list this wrote.
        if n > self.0.len() {
            return None;
        }
        (0..n).map(|_| each(self)).collect()
    }
    fn slots(&mut self) -> Option<Vec<Slot>> {
        self.list(|r| match r.byte()? {
            0 => Some(Slot::I32),
            1 => Some(Slot::I64),
            2 => Some(Slot::F64),
            _ => None,
        })
    }
    fn ty(&mut self) -> Option<Ty> {
        Some(match self.byte()? {
            0 => Ty::Int,
            1 => Ty::Float,
            2 => Ty::Bool,
            3 => Ty::Str,
            4 => Ty::Node,
            5 => Ty::Void,
            6 => Ty::Any,
            7 => Ty::Array(Box::new(self.ty()?)),
            8 => Ty::Optional(Box::new(self.ty()?)),
            9 => Ty::Struct(self.str()?),
            10 => Ty::Fn(
                self.list(Reader::ty)?,
                Box::new(self.ty()?),
                self.list(Reader::bool)?,
            ),
            _ => return None,
        })
    }
    fn literal(&mut self) -> Option<Literal> {
        Some(match self.byte()? {
            0 => Literal::Int(self.u64()?.cast_signed()),
            1 => Literal::Float(f64::from_bits(self.u64()?)),
            2 => Literal::Bool(self.bool()?),
            3 => Literal::Str(self.str()?),
            _ => return None,
        })
    }
}

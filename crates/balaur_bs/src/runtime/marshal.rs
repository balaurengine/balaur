//! A runtime's values crossing into and out of module memory: encoding a
//! value as the raw slots a type takes, the defaults, taking a value back
//! out, and releasing what it held.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;

impl Runtime {
    // The module's own copy of `v`: strings and arrays are allocated in it.
    pub(super) fn encode(&mut self, ty: &Ty, v: &Value) -> Result<Vec<Raw>, RunError> {
        Ok(match (ty, v) {
            (Ty::Str, Value::Str(s)) => {
                let p = self.alloc(8 + s.len() as u32)?;
                let mut b = 1i32.to_le_bytes().to_vec();
                b.extend((s.len() as u32).to_le_bytes());
                b.extend(s.as_bytes());
                self.wasm.write(p as usize, &b);
                vec![Raw::I32(p as i32)]
            }
            (Ty::Array(et), Value::Array(items)) => {
                let esz = 8 * width(&self.meta, et);
                let data = if items.is_empty() {
                    0
                } else {
                    self.alloc((items.len() * esz) as u32)?
                };
                for (i, it) in items.iter().enumerate() {
                    let b: Vec<u8> = self
                        .encode(et, it)?
                        .into_iter()
                        .flat_map(Raw::bytes)
                        .collect();
                    self.wasm.write(data as usize + i * esz, &b);
                }
                let h = self.alloc(16)?;
                let mut b = 1i32.to_le_bytes().to_vec();
                for x in [items.len() as u32, items.len() as u32, data] {
                    b.extend(x.to_le_bytes());
                }
                self.wasm.write(h as usize, &b);
                vec![Raw::I32(h as i32)]
            }
            (Ty::Any, v) => {
                let cell = values::any_cell(v, |t, v| {
                    Ok::<u32, RunError>(self.encode(t, v)?[0].i32() as u32)
                })?;
                let Some((tag, payload)) = cell else {
                    return Ok(vec![Raw::I32(0)]);
                };
                let p = self.alloc(crate::anys::CELL as u32)?;
                self.wasm
                    .write(p as usize, &values::cell_bytes(tag, payload));
                vec![Raw::I32(p as i32)]
            }
            (Ty::Optional(_), Value::Void) => values::zero(&self.meta, ty),
            (Ty::Optional(t), v) => {
                let mut raw = self.encode(t, v)?;
                raw.push(Raw::I32(1));
                raw
            }
            (Ty::Struct(s), Value::Struct(_, fields)) => {
                let decl = self.meta.fields(s).unwrap_or(&[]).to_vec();
                let mut raw = vec![];
                let defaults = self.meta.defaults(s).to_vec();
                for (n, t) in &decl {
                    let declared = defaults
                        .iter()
                        .find(|(k, _, _)| k == n)
                        .map(|(_, _, l)| literal(l));
                    match fields
                        .iter()
                        .find(|(k, _)| k == n)
                        .map(|(_, v)| v.clone())
                        .or(declared)
                    {
                        Some(fv) => raw.extend(self.encode(t, &fv)?),
                        None => raw.extend(self.default_of(t)?),
                    }
                }
                if self.meta.is_class(s) {
                    return Ok(vec![self.object_of(s, &raw)?]);
                }
                raw
            }
            _ => values::encode_plain(&self.meta, ty, v).map_err(|_| RunError {
                msg: format!("{v:?} is not a {}", show(ty)),
                line: None,
            })?,
        })
    }

    // A class object holding `raw`, counted once for its holder.
    pub(super) fn object_of(&mut self, s: &str, raw: &[Raw]) -> Result<Raw, RunError> {
        let p = self.alloc(values::object_size(&self.meta, s) as u32)?;
        let mut b = 1i32.to_le_bytes().to_vec();
        b.extend([0; 4]);
        b.extend(raw.iter().flat_map(|r| r.bytes()));
        self.wasm.write(p as usize, &b);
        Ok(Raw::I32(p as i32))
    }

    pub(super) fn default_of(&mut self, ty: &Ty) -> Result<Vec<Raw>, RunError> {
        match ty {
            Ty::Str => self.encode(ty, &Value::Str(String::new())),
            Ty::Array(_) => self.encode(ty, &Value::Array(vec![])),
            // A class is never null: a field of one starts as its defaults.
            Ty::Struct(s) if self.meta.is_class(s) => {
                self.encode(ty, &Value::Struct(s.clone(), vec![]))
            }
            Ty::Struct(s) if has_heap(&self.meta, ty) => {
                let decl = self.meta.fields(s).unwrap_or(&[]).to_vec();
                let mut raw = vec![];
                for (_, t) in &decl {
                    raw.extend(self.default_of(t)?);
                }
                Ok(raw)
            }
            _ => Ok(zero(&self.meta, ty)),
        }
    }

    // Reads a returned value and gives its references back.
    pub(super) fn take(&mut self, ty: &Ty, raw: &[Raw]) -> Value {
        let v = decode(&self.meta, &Live(&*self.wasm), ty, raw);
        if has_heap(&self.meta, ty) {
            self.release(ty, raw);
        }
        v
    }

    pub(super) fn release(&mut self, ty: &Ty, raw: &[Raw]) {
        for part in self.release_plan(ty).iter() {
            let r = raw[part.slot];
            match &part.dec {
                Some(name) => {
                    let _ = self.invoke(name, &[r], 0);
                }
                // No release export for the type: nothing in the module keeps one, so
                // the host made it; strings and plain arrays have no children to release.
                None => self.release_flat(&part.ty, r.i32()),
            }
        }
    }

    // What letting go of a `ty` releases, slot by slot: worked out once per
    // struct, since the host lets go of a script's copy after every call.
    pub(super) fn release_plan(&mut self, ty: &Ty) -> Rc<[host::Release]> {
        let key = match ty {
            Ty::Struct(name) => Some(name),
            _ => None,
        };
        let unit = self.unit;
        if let Some(plan) = key.and_then(|k| {
            self.host
                .borrow()
                .releases
                .get(unit)
                .and_then(|releases| releases.get(k).cloned())
        }) {
            return plan;
        }
        let plan: Rc<[host::Release]> = slots(&self.meta, ty)
            .into_iter()
            .enumerate()
            .filter_map(|(slot, (_, heap))| {
                let ty = heap?;
                let name = format!("{}{}", words::DEC, show(&ty));
                let dec = self.wasm.has_func_in(unit, &name).then_some(name);
                Some(host::Release { slot, ty, dec })
            })
            .collect();
        if let Some(k) = key {
            let mut host = self.host.borrow_mut();
            if host.releases.len() <= unit {
                host.releases.resize_with(unit + 1, Default::default);
            }
            host.releases[unit].insert(k.clone(), plan.clone());
        }
        plan
    }

    pub(super) fn release_flat(&mut self, ty: &Ty, p: i32) {
        if p == 0 {
            return;
        }
        if let Ty::Struct(s) = ty {
            let live = Live(&*self.wasm);
            if rd32(&live, p as usize) != 1 {
                // Someone else holds it too: one count less.
                let n = rd32(&live, p as usize) as i32;
                if n > 1 {
                    self.wasm.write(p as usize, &(n - 1).to_le_bytes());
                }
                return;
            }
            let decl = self.meta.fields(s).unwrap_or(&[]).to_vec();
            let mut at = p as usize + values::OBJ_HEADER;
            let mut fields = vec![];
            for (_, t) in &decl {
                let raw: Vec<Raw> = slots(&self.meta, t)
                    .iter()
                    .enumerate()
                    .map(|(j, (sl, _))| slot_at(&live, *sl, at + 8 * j))
                    .collect();
                at += 8 * raw.len();
                fields.push((t.clone(), raw));
            }
            for (t, raw) in fields {
                self.release(&t, &raw);
            }
            let _ = self.invoke(words::FREE, &[Raw::I32(p)], 0);
            return;
        }
        if let Ty::Array(et) = ty {
            let live = Live(&*self.wasm);
            let (len, data) = (rd32(&live, p as usize + 4), rd32(&live, p as usize + 12));
            let esz = 8 * width(&self.meta, et) as u32;
            let elements: Vec<Vec<Raw>> = (0..len)
                .map(|i| {
                    slots(&self.meta, et)
                        .iter()
                        .enumerate()
                        .map(|(j, (s, _))| slot_at(&live, *s, (data + i * esz) as usize + 8 * j))
                        .collect()
                })
                .collect();
            for raw in elements {
                self.release(et, &raw);
            }
            if data != 0 {
                let _ = self.invoke(words::FREE, &[Raw::I32(data as i32)], 0);
            }
        }
        let _ = self.invoke(words::FREE, &[Raw::I32(p)], 0);
    }
}

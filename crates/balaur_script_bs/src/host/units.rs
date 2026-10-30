// A script built as units, each file compiled alone once and linked with
// the units of the modules it uses when it loads.

use super::{BsHost, CACHE_DIR, File, Home, METER, script_type};
use crate::api;
use anyhow::{Result, anyhow};
use balaur_bs::{Compiled, Options};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

impl BsHost {
    /// Built as units: every file compiled alone once, a script linked with
    /// the units of the modules it uses when it loads. A debugger's probes,
    /// a profiler's counters, a browser's metering and a native build each
    /// want the whole file in one module, as does `BALAUR_BS_UNITS=0`.
    pub(super) fn as_units(&self, key: &str) -> bool {
        !METER
            && !crate::native::any_native()
            && !self.probed(key)
            && !self.inner.profiling.get()
            && std::env::var("BALAUR_BS_UNITS").map_or(true, |v| v != "0")
    }

    /// The file at `key` built as a unit, once until an edit, and kept on
    /// disk between runs.
    pub(super) fn unit(&self, key: &str) -> Result<Arc<Compiled>> {
        if let Some(u) = self.inner.units.borrow().get(key) {
            return Ok(u.clone());
        }
        let source = self
            .kept_source(key)
            .ok_or_else(|| anyhow!("{key}: no such file"))?;
        let u = if let Some(u) = self.kept_unit(key, &source) {
            u
        } else {
            let u = self.unit_of(key, &source)?;
            self.keep_unit(key, &source, &u);
            u
        };
        let u = Arc::new(u);
        self.inner
            .units
            .borrow_mut()
            .insert(key.to_string(), u.clone());
        Ok(u)
    }

    // Where the unit built from `source`, the file at `key`, is kept: named
    // by them and the engine's API. `None` for a pack, which keeps nothing.
    pub(super) fn unit_path(&self, key: &str, source: &str) -> Option<PathBuf> {
        if self.inner.pack.is_some() {
            return None;
        }
        let api = match self.inner.api_hash.take() {
            Some(h) => h,
            None => balaur_bs::module_hash(api::lines(&self.api()).join("\n").as_bytes()),
        };
        self.inner.api_hash.set(Some(api.clone()));
        let name = balaur_bs::module_hash(format!("{api}\n{key}\n{source}").as_bytes());
        let dir = balaur_core::engine_api::user_data_dir_of(&self.inner.engine).join(CACHE_DIR);
        Some(dir.join(format!("{name}.unit")))
    }

    // The unit kept for `source`, while every module it read declares what it
    // did then: each one's key and declarations' hash, then the build.
    pub(super) fn kept_unit(&self, key: &str, source: &str) -> Option<Compiled> {
        let bytes = std::fs::read(self.unit_path(key, source)?).ok()?;
        let text = std::str::from_utf8(bytes.split(|b| *b == 0).next()?).ok()?;
        for line in text.lines() {
            let (dep, hash) = line.rsplit_once(' ')?;
            let dep_source = self.kept_source(dep)?;
            let now = self.inner.parsed.declarations(dep, &dep_source).ok()?;
            if format!("{now:016x}") != hash {
                return None;
            }
        }
        Compiled::from_bytes(&bytes[text.len() + 1..])
    }

    pub(super) fn keep_unit(&self, key: &str, source: &str, unit: &Compiled) {
        let Some(path) = self.unit_path(key, source) else {
            return;
        };
        let mut text = String::new();
        for dep in &unit.meta.uses {
            let Some(hash) = self.inner.parsed.known(dep) else {
                return;
            };
            let _ = writeln!(text, "{dep} {hash:016x}");
        }
        let mut bytes = text.into_bytes();
        bytes.push(0);
        bytes.extend(unit.to_bytes());
        let partial = path.with_extension("partial");
        let kept = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&partial, &bytes))
            .and_then(|()| std::fs::rename(&partial, &path));
        if let Err(e) = kept {
            tracing::debug!("{key}: keeping its unit: {e}");
        }
    }

    pub(super) fn unit_of(&self, key: &str, source: &str) -> Result<Compiled> {
        balaur_bs::compile(source, &self.unit_options(key)).map_err(|e| anyhow!("{key}:{e}"))
    }

    pub(super) fn unit_options(&self, key: &str) -> Options {
        Options {
            host: self.api(),
            modules: Some(self.modules()),
            path: key.to_string(),
            unit: true,
            parsed: Some(self.inner.parsed.clone()),
            ..Options::default()
        }
    }

    // The script's unit with the units of every module it uses, near or far.
    pub(super) fn compile_units(&self, key: &str) -> Result<(Compiled, String)> {
        let root = self.unit(key)?;
        let mut linked: BTreeMap<String, Arc<Compiled>> = BTreeMap::new();
        let mut queue: Vec<String> = root.meta.uses.clone();
        while let Some(k) = queue.pop() {
            if k == key || linked.contains_key(&k) {
                continue;
            }
            let u = self.unit(&k)?;
            queue.extend(u.meta.uses.iter().cloned());
            linked.insert(k, u);
        }
        let mut compiled = (*root).clone();
        compiled.linked = linked.into_iter().collect();
        let ty = script_type(key, &compiled.meta);
        Ok((compiled, ty))
    }

    /// The project's instance made again: every file of it still built as
    /// units, and each of `changed` that is, linked into a fresh one, and
    /// every instance of them carried across by field name. Nothing changes
    /// when a file does not build.
    pub(super) fn rebuild_project(&self, changed: &[String]) -> Result<()> {
        let project = self.inner.project.borrow().clone();
        let members: Vec<(String, Rc<File>)> = self
            .inner
            .files
            .borrow()
            .iter()
            .filter(|(key, file)| {
                self.as_units(key) && (changed.contains(key) || file.in_project(project.as_ref()))
            })
            .map(|(key, file)| (key.clone(), file.clone()))
            .collect();
        if members.is_empty() {
            return Ok(());
        }
        // Asked by a script of the instance while it runs: built now, so what
        // breaks is said now, and swapped in between frames.
        if members
            .iter()
            .any(|(_, file)| file.home.rt.try_borrow_mut().is_err())
        {
            for key in changed {
                self.compile_module(key)?;
            }
            let mut rebuild = self.inner.rebuild.borrow_mut();
            for key in changed {
                if !rebuild.contains(key) {
                    rebuild.push(key.clone());
                }
            }
            return Ok(());
        }
        let mut built = vec![];
        for (key, _) in &members {
            let (compiled, ty) = self.compile_module(key)?;
            if ty.is_empty() {
                let class = balaur_bs::script_class(key);
                return Err(anyhow!("{key}: it declares no `pub class {class}`"));
            }
            built.push((key.clone(), compiled, ty));
        }
        let mut fresh = self.project_runtime()?;
        let mut units = vec![];
        for (key, compiled, _) in &built {
            let unit = self.unit(key)?;
            let index = fresh
                .link(key, &unit, &compiled.linked)
                .map_err(|e| anyhow!("{key}: {e}"))?;
            units.push(index);
        }
        let unit_of = |path: &str| {
            built
                .iter()
                .position(|(key, ..)| key == path)
                .map(|at| units[at])
        };
        // Out of each home a member ran in: the project's, or a file's own.
        let mut homes: Vec<Rc<Home>> = vec![];
        for (_, file) in &members {
            if !homes.iter().any(|home| Rc::ptr_eq(home, &file.home)) {
                homes.push(file.home.clone());
            }
        }
        for home in &homes {
            let paths: Vec<&str> = members
                .iter()
                .filter(|(_, file)| Rc::ptr_eq(&file.home, home))
                .map(|(key, _)| key.as_str())
                .collect();
            self.carry(home, &mut fresh, |path| {
                if paths.contains(&path) {
                    unit_of(path)
                } else {
                    None
                }
            })?;
        }
        for (key, _) in &members {
            self.forget(key);
        }
        let home = Home::new(fresh);
        *self.inner.project.borrow_mut() = Some(home.clone());
        let mut files = self.inner.files.borrow_mut();
        for ((key, compiled, ty), unit) in built.into_iter().zip(units) {
            let file = File {
                home: home.clone(),
                unit,
                native: false,
                compiled,
                ty,
            };
            files.insert(key, Rc::new(file));
        }
        Ok(())
    }

    /// What an edit of `key` makes stale: its source and unit, and when what
    /// it declares changed, every unit built against that.
    pub(super) fn invalidate(&self, key: &str) {
        let before = self.inner.parsed.known(key);
        self.inner.parsed.forget(key);
        self.inner
            .sources
            .borrow_mut()
            .retain(|k, v| k != key && v.is_some());
        self.inner.units.borrow_mut().remove(key);
        let after = self
            .kept_source(key)
            .and_then(|s| self.inner.parsed.declarations(key, &s).ok());
        if before.is_some() && before != after {
            self.inner
                .units
                .borrow_mut()
                .retain(|_, u| !u.meta.uses.iter().any(|k| k == key));
        }
    }
}

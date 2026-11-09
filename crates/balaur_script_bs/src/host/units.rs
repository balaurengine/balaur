// Every file compiled alone once, and linked into its instance with the units
// of the modules it uses when it loads.

use super::{BsHost, CACHE_DIR, File, Home, METER, PLACES, Place, place_of, script_type};
use crate::api;
use anyhow::{Result, anyhow};
use balaur_bs::{Compiled, Options};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Numbers this process's writes of kept units, so no two share a temporary.
static UNIT_WRITES: AtomicU32 = AtomicU32::new(0);

/// What a unit is built with, past what it says: the build each file's unit is
/// kept under, so turning a debugger or a profiler off costs nothing.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct Build {
    /// A debugger's probes.
    debug: bool,
    /// A profiler's counters.
    profile: bool,
}

impl Build {
    pub(super) const PLAIN: Build = Build {
        debug: false,
        profile: false,
    };

    fn name(self) -> &'static str {
        match (self.debug, self.profile) {
            (false, false) => "",
            (true, false) => "\nprobes",
            (false, true) => "\ncounters",
            (true, true) => "\nprobes\ncounters",
        }
    }
}

impl BsHost {
    /// How the file at `key` is built now.
    fn build_of(&self, key: &str) -> Build {
        Build {
            debug: self.probed(key),
            profile: self.inner.profiling.get(),
        }
    }

    /// Whether a native build may run the instance at `place`: it holds the
    /// plain units of the project's own files, and none built with probes or
    /// counters.
    pub(super) fn may_run_native(&self, place: Place) -> bool {
        place == Place::Project
            && !self.inner.skip_native.get()
            && !self.inner.profiling.get()
            && !self.inner.break_on_error.get()
            && !self.debugged_in(place)
    }

    /// Whether the unit at `key` can join the project's instance as it runs:
    /// any unit when the instance runs on its engine, and one a loaded
    /// native build has, with every unit it uses, when it runs as native
    /// code. A file the build lacks, changed since the build was made, moves
    /// the instance onto its engine for good: now, or between frames while a
    /// script runs, the file beside it until then.
    pub(super) fn joins_project(
        &self,
        key: &str,
        unit: &Compiled,
        linked: &[(String, Arc<Compiled>)],
    ) -> bool {
        #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
        crate::native::load_beside_executable();
        if !self.instance_native() {
            return true;
        }
        let missing = std::iter::once((key, unit))
            .chain(linked.iter().map(|(k, u)| (k.as_str(), &**u)))
            .find(|(_, u)| !crate::native::has_native(&u.wasm));
        let Some((file, u)) = missing else {
            return true;
        };
        tracing::warn!(
            "{file}: the native build has no module {}; the project's scripts run on their engine",
            balaur_bs::module_hash(&u.wasm)
        );
        self.inner.skip_native.set(true);
        if self.inner.project.borrow().is_none() {
            return true;
        }
        if let Err(e) = self.rebuild_instance(Place::Project, &[]) {
            tracing::error!("{file}: moving the project's scripts off native code: {e:#}");
        }
        if self.instance_native() {
            self.inner.rebuild.borrow_mut().push(key.to_string());
            return false;
        }
        true
    }

    /// Whether the project's instance runs as native code: the one there is,
    /// or the one the first file to join would make.
    fn instance_native(&self) -> bool {
        if let Some(home) = self.inner.project.borrow().as_ref() {
            return home.native;
        }
        self.may_run_native(Place::Project)
            && self
                .project_unit()
                .is_ok_and(|root| crate::native::has_native(&root.wasm))
    }

    /// The file at `key` built as a unit as it is built now, once until an
    /// edit, and kept on disk between runs.
    pub(super) fn unit(&self, key: &str) -> Result<Arc<Compiled>> {
        let build = self.build_of(key);
        let kept = self.inner.units.borrow();
        if let Some(u) = kept.get(key).and_then(|builds| builds.get(&build)) {
            return Ok(u.clone());
        }
        drop(kept);
        let source = self
            .kept_source(key)
            .ok_or_else(|| anyhow!("{key}: no such file"))?;
        let u = if let Some(u) = self.kept_unit(key, &source, build) {
            u
        } else {
            let u = balaur_bs::compile(&source, &self.options_of(key, build))
                .map_err(|e| anyhow!("{key}:{e}"))?;
            self.keep_unit(key, &source, build, &u);
            u
        };
        let u = Arc::new(u);
        self.inner
            .units
            .borrow_mut()
            .entry(key.to_string())
            .or_default()
            .insert(build, u.clone());
        Ok(u)
    }

    // Where the unit built from `source`, the file at `key`, is kept: named
    // by them, its build and the engine's API. `None` for a pack or a page,
    // which keep nothing.
    fn unit_path(&self, key: &str, source: &str, build: Build) -> Option<PathBuf> {
        if self.inner.pack.is_some() || cfg!(target_arch = "wasm32") {
            return None;
        }
        let api = match self.inner.api_hash.take() {
            Some(h) => h,
            None => balaur_bs::module_hash(api::compiled_against(&self.api()).as_bytes()),
        };
        self.inner.api_hash.set(Some(api.clone()));
        let build = build.name();
        let name = balaur_bs::module_hash(format!("{api}\n{key}{build}\n{source}").as_bytes());
        let dir = balaur_core::engine_api::user_data_dir_of(&self.inner.engine).join(CACHE_DIR);
        Some(dir.join(format!("{name}.unit")))
    }

    // The unit kept for `source`, while every module it read declares what it
    // did then: each one's key and declarations' hash, then the build.
    fn kept_unit(&self, key: &str, source: &str, build: Build) -> Option<Compiled> {
        let bytes = std::fs::read(self.unit_path(key, source, build)?).ok()?;
        let text = std::str::from_utf8(bytes.split(|b| *b == 0).next()?).ok()?;
        for line in text.lines() {
            let (dep, hash) = line.rsplit_once(' ')?;
            let dep_source = self.kept_source(dep)?;
            let now = self.inner.parsed.declarations(dep, &dep_source).ok()?;
            if format!("{now:016x}") != hash {
                return None;
            }
        }
        Compiled::from_bytes(bytes.get(text.len() + 1..)?)
    }

    fn keep_unit(&self, key: &str, source: &str, build: Build, unit: &Compiled) {
        let Some(path) = self.unit_path(key, source, build) else {
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
        // Processes sharing a data folder build the same unit at once: one
        // renaming a temporary another is truncating would keep an empty file.
        let write = UNIT_WRITES.fetch_add(1, Ordering::Relaxed);
        let partial = path.with_extension(format!("{}-{write}.partial", std::process::id()));
        let kept = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&partial, &bytes))
            .and_then(|()| std::fs::rename(&partial, &path));
        if let Err(e) = kept {
            tracing::debug!("{key}: keeping its unit: {e}");
        }
    }

    /// `source` as the unit of the file at `key` that ships: never with a
    /// debugger's probes or a profiler's counters.
    pub(crate) fn unit_of(&self, key: &str, source: &str) -> Result<Compiled> {
        balaur_bs::compile(source, &self.options_of(key, Build::PLAIN))
            .map_err(|e| anyhow!("{key}:{e}"))
    }

    /// How the file at `key` compiles as it is built now.
    pub(super) fn unit_options(&self, key: &str) -> Options {
        self.options_of(key, self.build_of(key))
    }

    /// How the file at `key` compiles as `build`. A browser's engine meters no
    /// fuel and is wired straight to the script, whatever else it is built with.
    pub(super) fn options_of(&self, key: &str, build: Build) -> Options {
        Options {
            debug: build.debug,
            profile: build.profile,
            meter: METER,
            direct_imports: METER,
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

    /// Whether what runs at `place` is not what its files are built as now, or
    /// on the engine they now ask for.
    pub(super) fn stale(&self, place: Place) -> bool {
        let home = self.place_slot(place).borrow().clone();
        let engine_moved = home.is_some_and(|home| {
            home.parking != self.debugged_in(place) || (home.native && !self.may_run_native(place))
        });
        engine_moved
            || self.inner.files.borrow().iter().any(|(key, file)| {
                file.place == place
                    && (std::iter::once((key.as_str(), &file.compiled.meta))
                        .chain(
                            file.compiled
                                .linked
                                .iter()
                                .map(|(k, u)| (k.as_str(), &u.meta)),
                        )
                        .any(|(unit, meta)| {
                            let build = self.build_of(unit);
                            meta.debug != build.debug || meta.profiled != build.profile
                        }))
            })
    }

    /// The instance at `place` made again if [`Self::stale`], its objects
    /// carried across, its compiled files reused.
    pub(super) fn rebuild_stale(&self) {
        for place in PLACES {
            if self.stale(place)
                && let Err(e) = self.rebuild_instance(place, &[])
            {
                tracing::error!("{e:#}");
            }
        }
    }

    /// The instance at `place` made again: every file of it, and each of
    /// `changed` that is at that place or to be, linked into a fresh one, and
    /// every object of them carried across by field name. Nothing changes
    /// when a file does not build. False when it has to wait: a script of the
    /// instance runs or is stopped, and it is built between frames.
    /// A fresh instance at `place` with each of `built` linked into it, and
    /// the unit each became.
    fn linked_fresh(
        &self,
        place: Place,
        built: &[(String, Compiled, String)],
    ) -> Result<(balaur_bs::runtime::Runtime, Vec<usize>)> {
        let unit_builds = built
            .iter()
            .map(|(key, ..)| self.unit(key))
            .collect::<Result<Vec<_>>>()?;
        let joining: Vec<&Compiled> =
            unit_builds
                .iter()
                .map(|unit| &**unit)
                .chain(built.iter().flat_map(|(_, compiled, _)| {
                    compiled.linked.iter().map(|(_, linked)| &**linked)
                }))
                .collect();
        let mut fresh = self.instance_runtime(place, &joining)?;
        let mut units = vec![];
        for ((key, compiled, _), unit) in built.iter().zip(&unit_builds) {
            let index = fresh
                .link(key, unit, &compiled.linked)
                .map_err(|e| anyhow!("{key}: {e}"))?;
            units.push(index);
        }
        Ok((fresh, units))
    }

    pub(super) fn rebuild_instance(&self, place: Place, changed: &[String]) -> Result<bool> {
        let members: Vec<(String, Rc<File>)> = self
            .inner
            .files
            .borrow()
            .iter()
            .filter(|(key, file)| file.place == place || changed.contains(key))
            .map(|(key, file)| (key.clone(), file.clone()))
            .collect();
        if members.is_empty() {
            return Ok(true);
        }
        let stopped = self.holds_instance(place);
        let running = members
            .iter()
            .any(|(_, file)| file.home.rt.try_borrow_mut().is_err());
        if stopped || running {
            // Asked by a script of the instance while it runs: built now, so
            // what breaks is said now, and swapped in between frames.
            if running && !stopped {
                for key in changed {
                    self.compile_units(key)?;
                }
            }
            let mut rebuild = self.inner.rebuild.borrow_mut();
            let every = members.iter().map(|(key, _)| key);
            for key in if changed.is_empty() {
                every.collect::<Vec<_>>()
            } else {
                changed.iter().collect()
            } {
                if !rebuild.contains(key) {
                    rebuild.push(key.clone());
                }
            }
            return Ok(false);
        }
        let mut built = vec![];
        for (key, _) in &members {
            let (compiled, ty) = self.compile_units(key)?;
            if ty.is_empty() {
                let class = balaur_bs::script_class(key);
                return Err(anyhow!("{key}: it declares no `pub class {class}`"));
            }
            built.push((key.clone(), compiled, ty));
        }
        let (mut fresh, units) = self.linked_fresh(place, &built)?;
        let unit_of = |path: &str| {
            built
                .iter()
                .position(|(key, ..)| key == path)
                .map(|at| units[at])
        };
        // Out of each home a member ran in: this place's, or the one beside it.
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
        let home = Home::new(fresh, self.debugged_in(place));
        self.arm(&home);
        *self.place_slot(place).borrow_mut() = Some(home.clone());
        let mut files = self.inner.files.borrow_mut();
        for ((key, compiled, ty), unit) in built.into_iter().zip(units) {
            let file = File {
                native: home.native,
                home: home.clone(),
                place,
                unit,
                compiled,
                ty,
            };
            files.insert(key, Rc::new(file));
        }
        Ok(true)
    }

    /// Each of `keys`' instances made again: what a reload names, once each.
    pub(super) fn rebuild_keys(&self, keys: &[String]) {
        for place in PLACES {
            let at: Vec<String> = keys
                .iter()
                .filter(|key| place_of(key) == place)
                .cloned()
                .collect();
            if at.is_empty() {
                continue;
            }
            match self.rebuild_instance(place, &at) {
                Ok(true) => tracing::info!("reloaded {}", at.join(", ")),
                Ok(false) => {}
                Err(e) => {
                    tracing::error!("{e:#}");
                    self.inner
                        .reload_owed
                        .borrow_mut()
                        .retain(|key| !at.contains(key));
                }
            }
        }
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
            self.inner.units.borrow_mut().retain(|_, builds| {
                builds.retain(|_, u| !u.meta.uses.iter().any(|k| k == key));
                !builds.is_empty()
            });
        }
    }
}

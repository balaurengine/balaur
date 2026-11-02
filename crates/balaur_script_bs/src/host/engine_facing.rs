//! What the engine reaches a `.bs` host through: `ScriptCompiler` to build a
//! file, `ScriptHost` to run, reload, save and debug the scripts on nodes.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;

impl ScriptCompiler for BsHost {
    fn extensions(&self) -> &[&str] {
        &[EXTENSION]
    }

    fn compile(&self, rel: &str, source: &str) -> Result<Vec<u8>> {
        let (compiled, _) = self.compile_source(&key_of(rel), source)?;
        self.inner
            .called
            .borrow_mut()
            .extend(compiled.meta.host_calls.iter().map(api::line));
        Ok(source.as_bytes().to_vec())
    }

    fn uses(&self, rel: &str, source: &str) -> Result<Vec<String>> {
        let key = key_of(rel);
        let options = Options {
            modules: Some(self.modules()),
            path: key.clone(),
            ..Options::default()
        };
        balaur_bs::uses(source, &options).map_err(|e| anyhow!("{key}:{e}"))
    }

    fn api(&self) -> Vec<String> {
        self.inner.called.borrow().iter().cloned().collect()
    }
}

impl ScriptHost<Engine> for BsHost {
    fn module(&self, name: &str) -> Result<Module<Engine>> {
        Ok(Module::new(name, self.inner.registry.declared.clone()))
    }

    fn attach_with_props(&self, node: NodeId, path: &str, props: &[(String, Value)]) -> Result<()> {
        let file = self.file(path)?;
        let mut init: Vec<(String, L)> = vec![];
        for (name, v) in props {
            match file.field_ty(name) {
                Some(t) => init.push((name.clone(), from_neutral(v, &t, &file.compiled.meta))),
                None => {
                    tracing::warn!("[{path}] property '{name}' is set on a node but not exported");
                }
            }
        }
        if file.field_ty(NODE_FIELD) == Some(Ty::Node) {
            init.push((NODE_FIELD.into(), L::Node(node.0.cast_signed())));
        }
        let init_ref: Vec<(&str, L)> = init.iter().map(|(n, v)| (n.as_str(), v.clone())).collect();
        let obj = file
            .either(
                |rt| rt.in_unit(file.unit, |rt| rt.object(&file.ty, &init_ref)),
                |re| re.object(file.unit, &file.ty, &init_ref),
            )
            .map_err(|e| anyhow!("[{path}] {e}"))?;
        self.detach(node);
        self.inner.instances.borrow_mut().push(Rc::new(Instance {
            node: node.0,
            path: key_of(path),
            pointer: Cell::new(Runtime::pointer(&obj)),
            obj: RefCell::new(Some(obj)),
            lent: RefCell::new(None),
            gone: Cell::new(false),
        }));
        if self.inner.holds.get() > 0 {
            self.inner.held.borrow_mut().push(node.0);
        } else {
            self.init(node.0);
        }
        Ok(())
    }

    fn hold_inits(&self) {
        self.inner.holds.set(self.inner.holds.get() + 1);
    }

    fn release_inits(&self) {
        let depth = self.inner.holds.get().saturating_sub(1);
        self.inner.holds.set(depth);
        if depth == 0 {
            let held = std::mem::take(&mut *self.inner.held.borrow_mut());
            for node in held {
                self.init(node);
            }
        }
    }

    fn exports(&self, path: &str) -> Result<Vec<(String, Value)>> {
        let file = self.file(path)?;
        let meta = &file.compiled.meta;
        Ok(meta
            .defaults(&file.ty)
            .iter()
            .filter(|(_, public, _)| *public)
            .map(|(n, _, lit)| (n.clone(), spec(lit, &file.field_ty(n).unwrap_or(Ty::Void))))
            .collect())
    }

    fn detach(&self, node: NodeId) {
        let Some((file, inst)) = self.instance(node).or_else(|| self.parked_instance(node)) else {
            return;
        };
        if file.method("on_free").is_some() {
            let r = self.run(&file, &inst, "on_free", &[], None);
            report(r);
        }
        self.cancel(node.0);
        self.inner
            .instances
            .borrow_mut()
            .retain(|i| i.node != node.0);
        self.inner.parked.borrow_mut().retain(|i| i.node != node.0);
        inst.gone.set(true);
        if let Some(obj) = inst.obj.borrow_mut().take() {
            file.drop_object(obj);
        }
    }

    fn set_parked(&self, nodes: &[NodeId], parked: bool) {
        let named: balaur_core::collections::DetHashSet<u64> =
            nodes.iter().map(|node| node.0).collect();
        let inner = &self.inner;
        let (from, to, tasks_from, tasks_to) = if parked {
            (
                &inner.instances,
                &inner.parked,
                &inner.tasks,
                &inner.parked_tasks,
            )
        } else {
            (
                &inner.parked,
                &inner.instances,
                &inner.parked_tasks,
                &inner.tasks,
            )
        };
        let (moved, stayed): (Vec<_>, Vec<_>) = from
            .take()
            .into_iter()
            .partition(|inst| named.contains(&inst.node));
        *from.borrow_mut() = stayed;
        to.borrow_mut().extend(moved);
        let (moved, stayed): (Vec<_>, Vec<_>) = tasks_from
            .take()
            .into_iter()
            .partition(|task| named.contains(&task.node));
        *tasks_from.borrow_mut() = stayed;
        tasks_to.borrow_mut().extend(moved);
    }

    fn update(&self, dt: f32) {
        self.run_waiting_starts();
        // A task a busy instance could not take last time runs on here.
        if self.has_tasks() {
            self.run_tasks();
        }
        self.each("update", &[Value::Num(f64::from(dt))], true);
    }

    fn fixed_update(&self, dt: f32) {
        self.each("fixed_update", &[Value::Num(f64::from(dt))], true);
    }

    fn save_state(&self) -> Vec<(NodeId, Value)> {
        let mut all: Vec<Rc<Instance>> = self.inner.instances.borrow().clone();
        all.extend(self.inner.parked.borrow().iter().cloned());
        let mut out = vec![];
        for inst in all {
            let Some(file) = self.inner.files.borrow().get(&inst.path).cloned() else {
                continue;
            };
            let fields = inst
                .obj
                .borrow()
                .as_ref()
                .map(|o| file.home.rt.borrow_mut().fields(o))
                .unwrap_or_default();
            out.push((
                NodeId(inst.node),
                Value::Map(
                    fields
                        .iter()
                        .map(|(k, v)| (k.to_string(), to_neutral(v)))
                        .collect(),
                ),
            ));
        }
        out
    }

    fn load_state(&self, states: &[(NodeId, Value)]) {
        for (node, state) in states {
            let found = self.instance(*node).or_else(|| self.parked_instance(*node));
            let (Some((file, inst)), Value::Map(fields)) = (found, state) else {
                continue;
            };
            let mut guard = inst.obj.borrow_mut();
            let Some(obj) = guard.as_mut() else { continue };
            for (name, v) in fields {
                if let Some(t) = file.field_ty(name)
                    && let Err(e) = file.home.rt.borrow_mut().set(
                        obj,
                        name,
                        &from_neutral(v, &t, &file.compiled.meta),
                    )
                {
                    tracing::error!("[{}] load_state: {e}", inst.path);
                }
            }
        }
    }

    fn pump_reloads(&self) {
        let mut todo: Vec<String> = self.inner.rebuild.take();
        for key in self.edited() {
            self.invalidate(&key);
            // A module is no script of its own: what uses it reloads.
            let users: Vec<String> = self
                .inner
                .files
                .borrow()
                .iter()
                .filter(|(k, f)| {
                    **k == key
                        || f.compiled.meta.uses.contains(&key)
                        || f.compiled.linked.iter().any(|(l, _)| *l == key)
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in users {
                self.inner.reload_owed.borrow_mut().push(k.clone());
                if !todo.contains(&k) {
                    todo.push(k);
                }
            }
        }
        // Every file built as units reloads with the project, once.
        let (units, alone): (Vec<String>, Vec<String>) =
            todo.into_iter().partition(|key| self.as_units(key));
        if !units.is_empty() {
            match self.rebuild_project(&units) {
                Ok(()) => tracing::info!("reloaded {}", units.join(", ")),
                Err(e) => {
                    tracing::error!("{e:#}");
                    self.inner
                        .reload_owed
                        .borrow_mut()
                        .retain(|key| !units.contains(key));
                }
            }
        }
        for key in alone {
            match self.rebuild_key(&key) {
                Ok(()) => tracing::info!("reloaded {key}"),
                Err(e) => {
                    tracing::error!("{e:#}");
                    self.inner.reload_owed.borrow_mut().retain(|k| *k != key);
                }
            }
        }
        self.announce_reloads();
    }

    // A new module, with every instance of the file carried across by field
    // name: as a unit, the whole project's instance made again.
    fn reload(&self, key: &str) -> Result<()> {
        let key = key_of(key);
        self.inner.reload_owed.borrow_mut().push(key.clone());
        let built = self.rebuild_key(&key);
        if built.is_err() {
            self.inner.reload_owed.borrow_mut().retain(|k| *k != key);
        }
        self.announce_reloads();
        built
    }

    fn call_on(&self, node: NodeId, method: &str, args: &[Value]) -> Option<Value> {
        let (file, inst) = self.instance(node)?;
        let r = self.run(&file, &inst, method, args, None);
        report(r)
    }

    fn call_on_async(
        &self,
        node: NodeId,
        method: &str,
        args: &[Value],
        done: u64,
    ) -> Option<Value> {
        let Some((file, inst)) = self.instance(node) else {
            return Some(Value::Nil);
        };
        if file.method(method).is_none() {
            return Some(Value::Nil);
        }
        match self.run(&file, &inst, method, args, Some(done)) {
            Ok(out) => out,
            Err(e) => {
                tracing::error!("{e:#}");
                Some(Value::Nil)
            }
        }
    }

    fn has_method(&self, node: NodeId, method: &str) -> bool {
        self.instance(node)
            .is_some_and(|(f, _)| f.method(method).is_some())
    }

    fn script_field(&self, node: NodeId, name: &str) -> Option<Value> {
        let (file, inst) = self.instance(node)?;
        let obj = inst.obj.borrow();
        let obj = obj.as_ref()?;
        file.either(|rt| rt.get(obj, name), |re| re.get(obj, name))
            .map(|v| to_neutral(&v))
    }

    fn call_all(&self, method: &str) {
        self.each(method, &[], false);
    }

    fn call_all_with(&self, method: &str, args: &[Value]) {
        self.each(method, args, false);
    }

    fn announce(&self, method: &str, args: &[Value]) {
        self.tell(method, args, |_| true);
    }

    fn wake(&self, token: u64, payload: &Value) {
        self.deliver(token, payload);
    }

    fn scene_source(&self, rel: &str) -> Option<String> {
        let key = key_of(rel);
        if let Some(pack) = &self.inner.pack {
            return pack.scenes.get(&key).cloned();
        }
        let bytes = balaur_core::files::backend(&self.inner.engine)
            .read(&self.inner.root.join(&key))
            .ok()?;
        String::from_utf8(bytes).ok()
    }

    fn instance_count(&self) -> usize {
        self.inner.instances.borrow().len()
    }

    fn set_breakpoints(&self, path: &str, lines: &[usize]) -> Result<Vec<usize>> {
        self.debug_set_breakpoints(path, lines)
    }

    fn breakpoints(&self, path: &str) -> Vec<usize> {
        self.debug_breakpoints(path)
    }

    fn set_profiling(&self, on: bool) {
        self.profile_set(on);
    }

    fn script_costs(&self) -> Vec<(String, u64, u64)> {
        self.profile_scripts()
    }

    fn function_costs(&self) -> Vec<(String, u64, u64)> {
        self.profile_functions()
    }

    fn set_break_on_error(&self, on: bool) {
        self.debug_set_break_on_error(on);
    }

    fn break_on_error(&self) -> bool {
        self.debug_break_on_error()
    }

    fn request_break(&self) {
        self.debug_request_break();
    }

    fn paused(&self) -> Option<Pause> {
        self.debug_paused()
    }

    fn resume(&self, mode: StepMode) {
        self.debug_resume(mode);
    }

    fn tools_for(&self, path: &str) -> Option<&dyn balaur_script::LanguageTools> {
        BsHost::claims(path).then_some(self as &dyn balaur_script::LanguageTools)
    }
}

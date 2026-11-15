//! What the engine reaches a `.bs` host through: `ScriptCompiler` to build a
//! file, `ScriptHost` to run, reload, save and debug the scripts on nodes.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;
use super::exports;

const ON_FREE: &str = "on_free";

impl BsHost {
    /// A `Node` field's path, or each of a `[Node]` field's, as the node it
    /// names from `node`, read as `node.get_node` reads one. A path naming
    /// nothing is no node, and a warning naming the field and the path.
    fn nodes_named(
        &self,
        node: NodeId,
        (script, field): (&str, &str),
        v: &Value,
        ty: &Ty,
    ) -> Value {
        match (ty, v) {
            (Ty::Optional(inner), _) => self.nodes_named(node, (script, field), v, inner),
            (Ty::Node, Value::Str(at)) => {
                let found = balaur_core::entity_of(node).ok().and_then(|from| {
                    balaur_core::scene::find_node(&self.inner.engine.world(), from, at)
                });
                if let Some(found) = found {
                    return Value::Node(balaur_core::node_id_of(found).0);
                }
                tracing::warn!("[{script}] property '{field}' names no node at '{at}'");
                Value::Nil
            }
            (Ty::Array(inner), Value::List(items)) => Value::List(
                items
                    .iter()
                    .map(|item| self.nodes_named(node, (script, field), item, inner))
                    .collect(),
            ),
            _ => v.clone(),
        }
    }
}

impl ScriptCompiler for BsHost {
    fn extensions(&self) -> &[&str] {
        &[EXTENSION]
    }

    /// The file's unit, as it ships: a game template has no compiler.
    #[cfg(feature = "compile")]
    fn compile(&self, rel: &str, source: &str) -> Result<Vec<u8>> {
        let compiled = self.unit_of(&key_of(rel), source)?;
        self.inner
            .called
            .borrow_mut()
            .extend(compiled.meta.host_calls.iter().map(api::line));
        Ok(compiled.to_bytes())
    }

    #[cfg(not(feature = "compile"))]
    fn compile(&self, rel: &str, _source: &str) -> Result<Vec<u8>> {
        Err(anyhow!("{rel}: {}", super::units::NO_COMPILER))
    }

    #[cfg(feature = "compile")]
    fn uses(&self, rel: &str, source: &str) -> Result<Vec<String>> {
        let key = key_of(rel);
        let options = Options {
            modules: Some(self.modules()),
            path: key.clone(),
            ..Options::default()
        };
        balaur_bs::uses(source, &options).map_err(|e| anyhow!("{key}:{e}"))
    }

    #[cfg(not(feature = "compile"))]
    fn uses(&self, rel: &str, _source: &str) -> Result<Vec<String>> {
        Err(anyhow!("{rel}: {}", super::units::NO_COMPILER))
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
                Some(t) if links::class_of(&file, &t).is_some() && links::path_of(v).is_some() => {
                    self.note_link(node.0, name, links::path_of(v).unwrap_or_default());
                }
                Some(t) if matches!(v, Value::Map(_)) && exports::map_class(&file, &t).is_some() => {
                    let class = exports::map_class(&file, &t).unwrap_or_default();
                    if let Some(made) = exports::made_map(&file, &class, v, path) {
                        init.push((name.clone(), made));
                    }
                }
                Some(t) => {
                    let v = self.nodes_named(node, (path, name), v, &t);
                    init.push((name.clone(), from_neutral(&v, &t, &file.compiled.meta)));
                }
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
            // A field elsewhere may name this node, attached after its holder.
            self.point_all_links();
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
            // A restore respawns a target under a holder it left standing.
            self.point_all_links();
        }
    }

    fn exports(&self, path: &str) -> Result<Vec<(String, Value)>> {
        let file = self.file(path)?;
        Ok(super::exports::exports(&file, path))
    }

    // Read from the source, so a build without the compiler answers none.
    #[cfg(feature = "compile")]
    fn events(&self, path: &str) -> Vec<(String, String)> {
        let key = key_of(path);
        let Some(source) = self.kept_source(&key) else {
            return Vec::new();
        };
        self.inner
            .parsed
            .events(&key, &source)
            .unwrap_or_default()
            .into_iter()
            .map(|event| (event.name, event.doc))
            .collect()
    }

    fn detach(&self, node: NodeId) {
        let Some((file, inst)) = self.instance(node) else {
            return;
        };
        self.unlink(node.0);
        if file.method(ON_FREE).is_some() {
            let r = self.run(&file, &inst, ON_FREE, &[], None);
            report(r);
        }
        self.cancel(node.0);
        self.inner
            .instances
            .borrow_mut()
            .retain(|i| i.node != node.0);
        inst.gone.set(true);
        if let Some(obj) = inst.obj.borrow_mut().take() {
            file.drop_object(obj);
        }
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
        let all: Vec<Rc<Instance>> = self.inner.instances.borrow().clone();
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
                .unwrap_or_default()
                .into_iter()
                .filter(|(name, _)| !self.is_link(inst.node, name))
                .collect::<Vec<_>>();
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
            let found = self.instance(*node);
            let (Some((file, inst)), Value::Map(fields)) = (found, state) else {
                continue;
            };
            let mut guard = inst.obj.borrow_mut();
            let Some(obj) = guard.as_mut() else { continue };
            for (name, v) in fields {
                if self.is_link(node.0, name) {
                    continue;
                }
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
        if let Some(mode) = self.inner.resume_owed.take() {
            self.debug_resume(mode);
        }
        let mut todo: Vec<String> = self.inner.rebuild.take();
        self.queue_users_of(&self.edited(), &mut todo);
        // Every file of an instance reloads with it, once.
        self.rebuild_keys(&todo);
        self.announce_reloads();
    }

    // A new module, with every instance of the file carried across by field
    // name: its whole instance made again.
    fn reload(&self, keys: &[&str]) -> Result<()> {
        let keys: Vec<String> = keys.iter().map(|key| key_of(key)).collect();
        self.inner
            .reload_owed
            .borrow_mut()
            .extend(keys.iter().cloned());
        let built = self.rebuild_changed(&keys);
        if built.is_err() {
            let failed: std::collections::BTreeSet<&String> = keys.iter().collect();
            self.inner
                .reload_owed
                .borrow_mut()
                .retain(|key| !failed.contains(key));
        }
        self.announce_reloads();
        built
    }

    fn watch(&self, dir: &std::path::Path, key: &str, extensions: &[String]) -> bool {
        use notify::Watcher as _;
        let mut watcher = self.inner.watcher.borrow_mut();
        let Some(watcher) = watcher.as_mut() else {
            return false;
        };
        let mut asides = self.inner.watch_asides.borrow_mut();
        if asides.iter().any(|aside| aside.at == dir) {
            return true;
        }
        match watcher.watch(dir, notify::RecursiveMode::Recursive) {
            Ok(()) => {
                asides.push(burst::Aside {
                    at: dir.to_path_buf(),
                    key: key.to_string(),
                    extensions: extensions.to_vec(),
                });
                true
            }
            Err(why) => {
                tracing::error!("watching {} for file edits: {why}", dir.display());
                false
            }
        }
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

    #[cfg(feature = "compile")]
    fn tools_for(&self, path: &str) -> Option<&dyn balaur_script::LanguageTools> {
        BsHost::claims(path).then_some(self as &dyn balaur_script::LanguageTools)
    }
}

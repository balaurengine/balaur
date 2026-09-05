//! The query functions both dimensions share: broad-phase readiness, filters,
//! hit shapes and the overlap readers.

macro_rules! functions {
    (state = $State:ty, vector = $vec:ident, dimensions = $N:literal, vocabulary = $vocabulary:expr) => {
        /// Make sure the broad phase's tree matches the colliders before asking it
        /// anything.
        ///
        /// Rapier fills the tree during a step. A game that raycasts in `init` — to
        /// drop a character onto the ground, say — has not stepped yet, and a game
        /// that spawns a wall and immediately asks what it hits has stepped, but not
        /// since. Both are ordinary, so both work: the tree is rebuilt here, once,
        /// when something has changed.
        pub(crate) fn ensure_queries(eng: &Engine) {
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            if state.queries_ready {
                return;
            }
            let state = &mut *state;
            let handles: Vec<ColliderHandle> =
                state.world.colliders.iter().map(|(h, _)| h).collect();
            let params = state.world.integration_parameters;
            let mut pairs = Vec::new();
            state.world.broad_phase.update(
                &params,
                &state.world.colliders,
                &state.world.bodies,
                &handles,
                &[],
                &mut pairs,
            );
            state.queries_ready = true;
        }

        /// The entity a collider belongs to, from the id put on it when it was
        /// inserted. One lookup rather than a scan of the whole collider map.
        pub(crate) fn entity_of_collider(collider: &Collider) -> Option<Entity> {
            Entity::from_bits(collider.user_data as u64)
        }

        /// A `filter` table as rapier's `QueryFilter`.
        ///
        /// The predicate is deliberately absent here and applied by the caller: it
        /// needs the script host, and rapier's own predicate slot is a `&dyn Fn` with
        /// a lifetime this function cannot hand back.
        fn filter_of<'a>(
            opts: &Opts<'_>,
            groups: &'a mut Option<InteractionGroups>,
        ) -> QueryFilter<'a> {
            let filter = Opts(opts.get(k::FILTER));
            let mut flags = QueryFilterFlags::empty();
            match filter.text(k::ONLY) {
                Some(crate::vocabulary::words::DYNAMIC) => flags |= QueryFilterFlags::ONLY_DYNAMIC,
                Some(crate::vocabulary::words::KINEMATIC) => {
                    flags |= QueryFilterFlags::ONLY_KINEMATIC;
                }
                Some(crate::vocabulary::words::STATIC) => flags |= QueryFilterFlags::ONLY_FIXED,
                _ => {}
            }
            if !filter.boolean(k::SENSORS, true) {
                flags |= QueryFilterFlags::EXCLUDE_SENSORS;
            }
            if !filter.boolean(k::SOLIDS, true) {
                flags |= QueryFilterFlags::EXCLUDE_SOLIDS;
            }
            let mut out = QueryFilter::from(flags);
            if let Some(Value::List(items)) = filter.get(k::MASK) {
                let mut bits = 0u32;
                for item in items {
                    let layer = match item {
                        Value::Int(i) => Some(*i as u32),
                        Value::Num(n) => Some(*n as u32),
                        Value::Str(s) => s.parse().ok(),
                        _ => None,
                    };
                    if let Some(layer) = layer.filter(|l| *l < 32) {
                        bits |= 1 << layer;
                    }
                }
                *groups = Some(InteractionGroups::new(
                    Group::ALL,
                    Group::from_bits_truncate(if bits == 0 { u32::MAX } else { bits }),
                    InteractionTestMode::And,
                ));
                if let Some(groups) = groups.as_ref() {
                    out = out.groups(*groups);
                }
            }
            out
        }

        /// The nodes a filter's `exclude` and `exclude_body` name.
        ///
        /// Rapier filters by handle; a script names nodes. Excluding the caller's own
        /// node is the single most common thing a raycast needs, so it is worth the
        /// two lines it costs here rather than in every script.
        fn excluded(opts: &Opts<'_>, state: &$State) -> Vec<ColliderHandle> {
            let filter = Opts(opts.get(k::FILTER));
            let mut out = Vec::new();
            for key in [k::EXCLUDE, k::EXCLUDE_BODY] {
                let Some(bits) = filter.node(key) else {
                    continue;
                };
                let Some(entity) = Entity::from_bits(bits) else {
                    continue;
                };
                if let Some(handles) = state.colliders.get(&entity) {
                    out.extend(handles.iter().copied());
                }
            }
            out
        }

        /// Whether a script predicate accepts this hit.
        ///
        /// Called with the world *not* borrowed, so the predicate may ask the engine
        /// anything it likes — including another query.
        fn allowed(eng: &Engine, opts: &Opts<'_>, entity: Entity) -> Result<bool> {
            let Some(Value::Callback(id)) = opts
                .get(k::FILTER)
                .and_then(|f| Opts(Some(f)).get(k::PREDICATE))
            else {
                return Ok(true);
            };
            match eng.invoke(*id, &[Value::Node(entity.to_bits().get())])? {
                Value::Bool(false) | Value::Nil => Ok(false),
                _ => Ok(true),
            }
        }

        /// One hit, in the shape every query here returns.
        fn hit_value(entity: Entity, point: [f32; $N], normal: [f32; $N], distance: Real) -> Value {
            map([
                (k::NODE, Value::Node(entity.to_bits().get())),
                (k::POINT, Value::$vec(point)),
                (k::NORMAL, Value::$vec(normal)),
                ("distance", Value::Num(f64::from(distance))),
            ])
        }

        /// Sorted by distance, then by entity bits: two machines must agree on the
        /// order, and rapier's is its BVH's.
        fn sort_hits(hits: &mut [(Entity, Real, [f32; $N], [f32; $N])]) {
            // `total_cmp`, not `partial_cmp`: a NaN distance makes the latter a
            // non-order, which `sort_by` is allowed to panic on.
            hits.sort_by(|a, b| {
                a.1.total_cmp(&b.1)
                    .then_with(|| a.0.to_bits().cmp(&b.0.to_bits()))
            });
        }

        /// Node lists cross the seam sorted by entity bits, for the same reason hit
        /// lists cross it sorted by distance.
        fn node_list(hits: &mut Vec<Entity>) -> Value {
            hits.sort_unstable_by_key(|e| e.to_bits());
            hits.dedup();
            Value::List(
                hits.iter()
                    .map(|e| Value::Node(e.to_bits().get()))
                    .collect(),
            )
        }

        /// The `shape` half of a shapecast or shape query, in the collider
        /// vocabulary — so the shape you cast is spelled like the shape you attach.
        fn shape_params(opts: &Opts<'_>) -> Result<toml::Value> {
            let shape = opts.get(k::SHAPE).ok_or_else(|| {
                anyhow!(
                    "this query needs a `shape` table, in {}'s vocabulary",
                    $vocabulary
                )
            })?;
            balaur_core::node_api::to_toml(shape)
        }

        /// Nodes whose colliders intersect this node's, sorted by entity bits.
        ///
        /// Rapier tracks an intersection pair only when one side is a sensor, which is
        /// why this is not the same question as `shape_hits`.
        pub fn overlaps(eng: &Engine, entity: Entity) -> Vec<Entity> {
            let state = eng.resource::<$State>();
            let state = state.borrow();
            let Some(handles) = state.colliders.get(&entity) else {
                return Vec::new();
            };
            let mut hits = Vec::new();
            for &handle in handles {
                for (h1, _, h2, _, intersecting) in state.world.intersection_pairs_with(handle) {
                    if !intersecting {
                        continue;
                    }
                    let other = if h1 == handle { h2 } else { h1 };
                    if let Some(found) = entity_of_collider(&state.world.colliders[other]) {
                        if found != entity {
                            hits.push(found);
                        }
                    }
                }
            }
            hits.sort_unstable_by_key(|e| e.to_bits());
            hits.dedup();
            hits
        }

        /// `overlaps`, as a node list.
        pub(crate) fn overlaps_value(eng: &Engine, node: NodeId) -> Result<Vec<NodeId>> {
            Ok(overlaps(eng, entity_of(node)?)
                .into_iter()
                .map(node_id_of)
                .collect())
        }
    };
}
pub(crate) use functions;

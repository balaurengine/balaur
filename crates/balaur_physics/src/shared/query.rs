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

        /// A `filter` table as rapier's `QueryFilter`. A body kind it does not
        /// know is an error: a typo would otherwise filter nothing, silently.
        fn filter_of<'a>(
            opts: &Opts<'_>,
            groups: &'a mut Option<InteractionGroups>,
        ) -> Result<QueryFilter<'a>> {
            use crate::vocabulary::words as w;
            let filter = Opts(opts.get(k::FILTER));
            let kinds = filter.list(k::EXCLUDE_KINDS).unwrap_or_default();
            let mut words = Vec::with_capacity(kinds.len());
            for kind in kinds {
                match kind {
                    Value::Str(word)
                        if [w::STATIC, w::KINEMATIC, w::DYNAMIC].contains(&word.as_str()) =>
                    {
                        words.push(word.as_str());
                    }
                    other => return Err(not_a_body_kind(k::EXCLUDE_KINDS, other)),
                }
            }
            let bits = crate::vocabulary::flags::filter_bits(
                words,
                filter.boolean(k::HIT_SENSORS, true),
                filter.boolean(k::HIT_SOLIDS, true),
            );
            let mut flags = QueryFilterFlags::from_bits_truncate(bits);
            match filter.get(k::ONLY) {
                None | Some(Value::Nil) => {}
                Some(Value::Str(word)) if word == w::DYNAMIC => {
                    flags |= QueryFilterFlags::ONLY_DYNAMIC
                }
                Some(Value::Str(word)) if word == w::KINEMATIC => {
                    flags |= QueryFilterFlags::ONLY_KINEMATIC;
                }
                Some(Value::Str(word)) if word == w::STATIC => {
                    flags |= QueryFilterFlags::ONLY_FIXED
                }
                Some(other) => return Err(not_a_body_kind(k::ONLY, other)),
            }
            let mut out = QueryFilter::from(flags);
            if let Some(Value::List(items)) = filter.get(k::COLLISION_MASK) {
                let mut bits = 0u32;
                for item in items {
                    let layer = match item {
                        Value::Int(i) => Some(*i as u32),
                        Value::Num(n) => Some(*n as u32),
                        Value::Str(s) => s.parse().ok(),
                        _ => None,
                    };
                    if let Some(bit) = balaur_core::layers::bit(layer) {
                        bits |= bit;
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
            Ok(out)
        }

        fn not_a_body_kind(key: &str, given: &Value) -> anyhow::Error {
            use crate::vocabulary::words as w;
            anyhow!(
                "a filter's `{key}` names a body kind, one of `{}`, `{}` or `{}`; got {given:?}",
                w::STATIC,
                w::KINEMATIC,
                w::DYNAMIC
            )
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

        /// One hit, in the shape every query here returns.
        fn hit_value(entity: Entity, point: [f32; $N], normal: [f32; $N], distance: Real) -> Value {
            map([
                (k::NODE, Value::Node(entity.to_bits().get())),
                (k::POINT, Value::$vec(point)),
                (k::NORMAL, Value::$vec(normal)),
                ("distance", Value::Num(f64::from(distance))),
            ])
        }

        /// Sorted by distance, then by [`balaur_core::ids::order_key`]: two runs
        /// must agree on the order, and rapier's is its BVH's.
        fn sort_hits(
            hits: &mut [(Entity, Real, [f32; $N], [f32; $N])],
            world: &balaur_core::hecs::World,
        ) -> Result<()> {
            let keys = hits
                .iter()
                .map(|h| balaur_core::ids::order_key(world, h.0))
                .collect::<Result<Vec<_>>>()?;
            let mut order: Vec<usize> = (0..hits.len()).collect();
            // `total_cmp`, not `partial_cmp`: a NaN distance makes the latter a
            // non-order, which `sort_by` is allowed to panic on.
            order.sort_by(|&a, &b| {
                hits[a]
                    .1
                    .total_cmp(&hits[b].1)
                    .then_with(|| keys[a].cmp(&keys[b]))
            });
            let sorted: Vec<_> = order.iter().map(|&i| hits[i]).collect();
            hits.copy_from_slice(&sorted);
            Ok(())
        }

        /// Node lists cross the seam in [`balaur_core::ids::order_key`] order, for
        /// the same reason hit lists cross it sorted by distance.
        fn node_list(hits: &mut Vec<Entity>, world: &balaur_core::hecs::World) -> Result<Value> {
            balaur_core::ids::sort_by_id(world, hits)?;
            hits.dedup();
            Ok(Value::List(
                hits.iter()
                    .map(|e| Value::Node(e.to_bits().get()))
                    .collect(),
            ))
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

        /// Nodes whose colliders intersect this node's, in a stable order.
        ///
        /// Rapier tracks an intersection pair only when one side is a sensor, which is
        /// why this is not the same question as `overlap_shape`.
        pub fn overlaps(eng: &Engine, entity: Entity) -> Result<Vec<Entity>> {
            let state = eng.resource::<$State>();
            let state = state.borrow();
            let Some(handles) = state.colliders.get(&entity) else {
                return Ok(Vec::new());
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
            balaur_core::ids::sort_by_id(&eng.world(), &mut hits)?;
            hits.dedup();
            Ok(hits)
        }

        /// `overlaps`, as a node list.
        pub(crate) fn overlaps_value(eng: &Engine, node: NodeId) -> Result<Vec<NodeId>> {
            Ok(overlaps(eng, entity_of(node)?)?
                .into_iter()
                .map(node_id_of)
                .collect())
        }
    };
}
pub(crate) use functions;

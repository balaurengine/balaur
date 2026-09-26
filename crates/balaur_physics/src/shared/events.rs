//! The event plumbing both dimensions share: what a step collected, the order
//! it is delivered in, and the one mid-step rule that reads collider data.

use balaur_core::hecs::Entity;

/// Who a collider event is told to: the collider's node, and the node of the
/// body it hangs from when that is another one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Owner {
    pub(crate) node: Entity,
    pub(crate) body: Option<Entity>,
}

impl Owner {
    /// A collider's node, and its body's from the `user_data` that body holds.
    pub(crate) fn of(node: Entity, body_data: Option<u128>) -> Self {
        Self {
            node,
            body: body_data.and_then(|data| Entity::from_bits(data as u64)),
        }
    }
}

macro_rules! functions {
    (
        dimensions = $N:literal,
        towards = $towards:ident,
        axis = $axis:ident,
        normal = $normal:ty,
        decode = $decode:path
    ) => {
        /// One thing that happened, in Balaur's terms rather than rapier's handles.
        pub(crate) enum Event {
            Started(Owner, Owner),
            Stopped(Owner, Owner),
            Force(Owner, Owner, f32, [f32; $N]),
            /// The soft body, how many pieces, and the particle pairs torn.
            Tear(Entity, u32, Vec<[u32; 2]>),
        }

        impl Event {
            /// The pair and the kind, for sorting. Both sides are told, so the
            /// order within a pair does not matter; the order *between* events
            /// does, and a threaded step collects them in no particular order.
            /// The kind is in the key because one pair can raise a `Started`
            /// and a `Force` in one step.
            fn key(&self) -> (u64, u64, u8) {
                let (a, b, kind) = match self {
                    Self::Started(a, b) => (a.node, b.node, 0),
                    Self::Stopped(a, b) => (a.node, b.node, 1),
                    Self::Force(a, b, _, _) => (a.node, b.node, 2),
                    Self::Tear(a, _, _) => (*a, *a, 3),
                };
                (
                    a.to_bits().get().min(b.to_bits().get()),
                    a.to_bits().get().max(b.to_bits().get()),
                    kind,
                )
            }
        }

        /// Collects a step's events, from whichever thread raised them. The
        /// order they arrive in is not the order they are delivered in:
        /// [`Event::key`] is a total order, and `take` sorts by it.
        #[derive(Default)]
        pub(crate) struct Collector {
            events: Mutex<Vec<Event>>,
            /// The owners of colliders removed since the last step, whose
            /// contacts end during this one.
            gone: DetHashMap<ColliderHandle, Owner>,
        }

        impl Collector {
            pub(crate) fn after(gone: DetHashMap<ColliderHandle, Owner>) -> Self {
                Self {
                    events: Mutex::default(),
                    gone,
                }
            }

            /// Who is behind a collider handle: the ids stored on it and its
            /// body, or the owners it had when it was removed.
            fn owner(
                &self,
                bodies: &RigidBodySet,
                colliders: &ColliderSet,
                handle: ColliderHandle,
            ) -> Option<Owner> {
                owner_of(bodies, colliders, handle).or_else(|| self.gone.get(&handle).copied())
            }

            pub(crate) fn take(self) -> Vec<Event> {
                let mut events = self
                    .events
                    .into_inner()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                events.sort_unstable_by_key(Event::key);
                events
            }

            fn push(&self, event: Event) {
                self.events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(event);
            }
        }

        /// The collider's node and its body's, from the ids stored on both.
        fn owner_of(
            bodies: &RigidBodySet,
            colliders: &ColliderSet,
            handle: ColliderHandle,
        ) -> Option<Owner> {
            let collider = colliders.get(handle)?;
            let node = Entity::from_bits(collider.user_data as u64)?;
            let body = collider.parent().and_then(|b| bodies.get(b));
            Some(Owner::of(node, body.map(|b| b.user_data)))
        }

        impl EventHandler for Collector {
            fn handle_collision_event(
                &self,
                bodies: &RigidBodySet,
                colliders: &ColliderSet,
                event: CollisionEvent,
                _pair: Option<&ContactPair>,
            ) {
                let (Some(a), Some(b)) = (
                    self.owner(bodies, colliders, event.collider1()),
                    self.owner(bodies, colliders, event.collider2()),
                ) else {
                    return;
                };
                self.push(if event.started() {
                    Event::Started(a, b)
                } else {
                    Event::Stopped(a, b)
                });
            }

            fn handle_contact_force_event(
                &self,
                dt: crate::scalar::Real,
                bodies: &RigidBodySet,
                colliders: &ColliderSet,
                pair: &ContactPair,
                total_force_magnitude: crate::scalar::Real,
            ) {
                let event = ContactForceEvent::from_contact_pair(dt, pair, total_force_magnitude);
                let (Some(a), Some(b)) = (
                    owner_of(bodies, colliders, event.collider1),
                    owner_of(bodies, colliders, event.collider2),
                ) else {
                    return;
                };
                let d = event.max_force_direction;
                self.push(Event::Force(
                    a,
                    b,
                    crate::scalar::f32_of(event.total_force_magnitude),
                    crate::scalar::$axis(d),
                ));
            }

            fn handle_soft_body_tear_event(
                &self,
                soft_bodies: &SoftBodySet,
                event: &SoftBodyTearEvent,
            ) {
                let Some(entity) = soft_bodies
                    .get(event.soft_body)
                    .and_then(|body| Entity::from_bits(body.user_data as u64))
                else {
                    return;
                };
                let pieces = event.pieces.len() as u32;
                self.push(Event::Tear(entity, pieces, event.torn_edges.clone()));
            }
        }

        /// The method a script implements for each event, and the arguments it
        /// gets.
        fn dispatch(eng: &Engine, event: &Event) {
            let node = |e: Entity| Value::Node(e.to_bits().get());
            // The collider's node first, then the body it hangs from.
            let tell = |at: Owner, name: &str, payload: Value| {
                if let Some(body) = at.body.filter(|&body| body != at.node) {
                    balaur_core::events::announce(eng, at.node, name, payload.clone());
                    balaur_core::events::announce(eng, body, name, payload);
                } else {
                    balaur_core::events::announce(eng, at.node, name, payload);
                }
            };
            match event {
                Event::Started(a, b) => {
                    tell(*a, hook::COLLISION_ENTER, node(b.node));
                    tell(*b, hook::COLLISION_ENTER, node(a.node));
                }
                Event::Stopped(a, b) => {
                    tell(*a, hook::COLLISION_EXIT, node(b.node));
                    tell(*b, hook::COLLISION_EXIT, node(a.node));
                }
                Event::Force(a, b, magnitude, direction) => {
                    let contact = |other: Owner| {
                        crate::vocabulary::map([
                            (k::OTHER, node(other.node)),
                            (k::FORCE, Value::Num(f64::from(*magnitude))),
                            (k::DIRECTION, Value::$towards(*direction)),
                        ])
                    };
                    tell(*a, hook::CONTACT_FORCE, contact(*b));
                    tell(*b, hook::CONTACT_FORCE, contact(*a));
                }
                Event::Tear(a, pieces, torn) => {
                    let edges = torn
                        .iter()
                        .map(|pair| Value::List(pair.map(|p| Value::Int(i64::from(p))).to_vec()))
                        .collect();
                    let tear = crate::vocabulary::map([
                        (k::PIECES, Value::Int(i64::from(*pieces))),
                        (k::TORN_EDGES, Value::List(edges)),
                    ]);
                    balaur_core::events::announce(eng, *a, hook::TEAR, tear);
                }
            }
        }

        /// Hand a step's events to the scripts that asked for them.
        ///
        /// Called with the dimension's state **not** borrowed: a handler is
        /// ordinary script code and may do anything, including move the body
        /// it was told about.
        pub(crate) fn deliver(eng: &Engine, events: &[Event]) {
            for event in events {
                dispatch(eng, event);
            }
        }

        /// The one mid-step rule left, and it reads collider data rather than
        /// calling a script.
        ///
        /// A hook runs on rapier's own threads, which is why it may not touch
        /// the `Engine`: that is what keeps `unsync-callbacks` off and the
        /// solver threaded.
        pub(crate) struct Hooks;

        impl PhysicsHooks for Hooks {
            fn modify_solver_contacts(&self, context: &mut ContactModificationContext<'_>) {
                // The one-way platform, which is what `one_way` on a collider
                // means: rapier owns the maths, we own the axis.
                if let Some(axis) = one_way_axis(context) {
                    context.update_as_oneway_platform(axis, 0.1);
                }
            }
        }

        /// The direction a one-way platform lets bodies through from,
        /// whichever of the pair is the platform.
        ///
        /// The axis rides in the high bits of the collider's `user_data`,
        /// beside the entity id: a hook runs while the state is borrowed by the
        /// step, so it cannot go and read the component. Six directions rather
        /// than a vector, because a platform's axis is a cardinal one in every
        /// game that has ever wanted this, and the encoding costs three bits.
        fn one_way_axis(context: &ContactModificationContext<'_>) -> Option<$normal> {
            let first = context.colliders.get(context.collider1)?;
            if let Some(axis) = $decode(first.user_data) {
                return Some(axis);
            }
            // The platform is the other collider: rapier reads the axis in the
            // first's frame, so it turns into that frame and reverses.
            let second = context.colliders.get(context.collider2)?;
            let axis = $decode(second.user_data)?;
            let world = second.position().rotation * axis;
            Some(-(first.position().rotation.inverse() * world))
        }
    };
}
pub(crate) use functions;

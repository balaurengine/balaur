//! The event plumbing both dimensions share: what a step collected, the order
//! it is delivered in, and the one mid-step rule that reads collider data.

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
            Started(Entity, Entity),
            Stopped(Entity, Entity),
            Force(Entity, Entity, f32, [f32; $N]),
        }

        impl Event {
            /// The pair and the kind, for sorting. Both sides are told, so the
            /// order within a pair does not matter; the order *between* events
            /// does, and a threaded step collects them in no particular order.
            /// The kind is in the key because one pair can raise a `Started`
            /// and a `Force` in one step.
            fn key(&self) -> (u64, u64, u8) {
                let (a, b, kind) = match self {
                    Self::Started(a, b) => (*a, *b, 0),
                    Self::Stopped(a, b) => (*a, *b, 1),
                    Self::Force(a, b, _, _) => (*a, *b, 2),
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
        }

        impl Collector {
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

        /// The entity behind a collider handle, from the id stored on it.
        fn entity_of(colliders: &ColliderSet, handle: ColliderHandle) -> Option<Entity> {
            Entity::from_bits(colliders.get(handle)?.user_data as u64)
        }

        impl EventHandler for Collector {
            fn handle_collision_event(
                &self,
                _bodies: &RigidBodySet,
                colliders: &ColliderSet,
                event: CollisionEvent,
                _pair: Option<&ContactPair>,
            ) {
                let (Some(a), Some(b)) = (
                    entity_of(colliders, event.collider1()),
                    entity_of(colliders, event.collider2()),
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
                _bodies: &RigidBodySet,
                colliders: &ColliderSet,
                pair: &ContactPair,
                total_force_magnitude: crate::scalar::Real,
            ) {
                let event = ContactForceEvent::from_contact_pair(dt, pair, total_force_magnitude);
                let (Some(a), Some(b)) = (
                    entity_of(colliders, event.collider1),
                    entity_of(colliders, event.collider2),
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
        }

        /// The method a script implements for each event, and the arguments it
        /// gets.
        fn dispatch(eng: &Engine, event: &Event) {
            let Some(host) = eng.script_host() else {
                return;
            };
            let node = |e: Entity| Value::Node(e.to_bits().get());
            match *event {
                Event::Started(a, b) => {
                    host.call_on(
                        balaur_core::node_id_of(a),
                        hook::ON_COLLISION_START,
                        &[node(b)],
                    );
                    host.call_on(
                        balaur_core::node_id_of(b),
                        hook::ON_COLLISION_START,
                        &[node(a)],
                    );
                }
                Event::Stopped(a, b) => {
                    host.call_on(
                        balaur_core::node_id_of(a),
                        hook::ON_COLLISION_STOP,
                        &[node(b)],
                    );
                    host.call_on(
                        balaur_core::node_id_of(b),
                        hook::ON_COLLISION_STOP,
                        &[node(a)],
                    );
                }
                Event::Force(a, b, magnitude, direction) => {
                    let force = Value::Num(f64::from(magnitude));
                    let towards = Value::$towards(direction);
                    host.call_on(
                        balaur_core::node_id_of(a),
                        hook::ON_CONTACT_FORCE,
                        &[node(b), force.clone(), towards.clone()],
                    );
                    host.call_on(
                        balaur_core::node_id_of(b),
                        hook::ON_CONTACT_FORCE,
                        &[node(a), force, towards],
                    );
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

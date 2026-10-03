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
        state = $State:ty,
        tear_record = $tear_record:path
    ) => {
        /// What the contact hook reads off a collider mid-step, in the
        /// collider's own frame. A side table rather than the component: the
        /// hook runs on rapier's threads while the step holds the state.
        #[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
        pub(crate) struct Surface {
            /// The way a one-way platform lets bodies through from, and how
            /// far from it, in radians, a contact may still hold.
            pub(crate) one_way: Option<($normal, crate::scalar::Real)>,
            /// How fast the surface slides along itself: a conveyor belt.
            pub(crate) velocity: $normal,
        }

        /// Every collider's [`Surface`], by handle. Only colliders that ask
        /// for the contact hook have a row.
        pub(crate) type Surfaces = DetHashMap<ColliderHandle, Surface>;

        /// One thing that happened, in Balaur's terms rather than rapier's handles.
        pub(crate) enum Event {
            Started(Owner, Owner, Touch),
            Stopped(Owner, Owner, Touch),
            Force(Owner, Owner, Push),
            /// The soft body, and what tore, read into a script's record at
            /// delivery: its moved joints name nodes the step cannot reach.
            Tear(Entity, SoftBodyTearEvent),
        }

        /// What a collision event says beyond the pair, from the first
        /// collider's side.
        pub(crate) struct Touch {
            /// Whether either collider is a sensor.
            sensor: bool,
            /// Whether the touch ended because a collider went away.
            removed: bool,
            /// On a start, each contact rapier holds: the point on the first
            /// collider, the point on the second, and the normal from the first
            /// towards the second, all in world space.
            contacts: Vec<([f32; $N], [f32; $N], [f32; $N])>,
        }

        /// What a contact force event measured, in Balaur's units.
        pub(crate) struct Push {
            /// The sum of every contact's force magnitude.
            magnitude: f32,
            /// The strongest contact's normal, from the first collider
            /// towards the second.
            direction: [f32; $N],
            /// Every contact's force, summed as vectors along those normals.
            total: [f32; $N],
            /// The strongest one contact's force.
            max: f32,
            /// Whether the force just rose past the threshold.
            started: bool,
        }

        impl Event {
            /// The pair and the kind, for sorting. Both sides are told, so the
            /// order within a pair does not matter; the order *between* events
            /// does, and a threaded step collects them in no particular order.
            /// The kind is in the key because one pair can raise a `Started`
            /// and a `Force` in one step.
            fn key(&self) -> (u64, u64, u8) {
                let (a, b, kind) = match self {
                    Self::Started(a, b, _) => (a.node, b.node, 0),
                    Self::Stopped(a, b, _) => (a.node, b.node, 1),
                    Self::Force(a, b, _) => (a.node, b.node, 2),
                    Self::Tear(a, _) => (*a, *a, 3),
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

        /// Every contact point a pair holds, on each collider, with its
        /// manifold's normal from the first collider towards the second.
        fn contacts_of(
            colliders: &ColliderSet,
            pair: &ContactPair,
        ) -> Vec<([f32; $N], [f32; $N], [f32; $N])> {
            let (Some(first), Some(second)) =
                (colliders.get(pair.collider1), colliders.get(pair.collider2))
            else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for manifold in pair.manifolds() {
                let normal = crate::scalar::$axis(manifold.data.normal);
                for point in &manifold.points {
                    out.push((
                        crate::scalar::$axis(first.position() * point.local_p1),
                        crate::scalar::$axis(second.position() * point.local_p2),
                        normal,
                    ));
                }
            }
            out
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
                pair: Option<&ContactPair>,
            ) {
                let (Some(a), Some(b)) = (
                    self.owner(bodies, colliders, event.collider1()),
                    self.owner(bodies, colliders, event.collider2()),
                ) else {
                    return;
                };
                let contacts = match pair {
                    Some(pair) if event.started() => contacts_of(colliders, pair),
                    _ => Vec::new(),
                };
                let touch = Touch {
                    sensor: event.sensor(),
                    removed: event.removed(),
                    contacts,
                };
                self.push(if event.started() {
                    Event::Started(a, b, touch)
                } else {
                    Event::Stopped(a, b, touch)
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
                self.push(Event::Force(
                    a,
                    b,
                    Push {
                        magnitude: crate::scalar::f32_of(event.total_force_magnitude),
                        direction: crate::scalar::$axis(event.max_force_direction),
                        total: crate::scalar::$axis(event.total_force),
                        max: crate::scalar::f32_of(event.max_force_magnitude),
                        started: event.started,
                    },
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
                self.push(Event::Tear(entity, event.clone()));
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
            // Each side hears its own points, and normals pointing away from it.
            let collision = |other: Owner, touch: &Touch, first: bool, start: bool| {
                let mut out = vec![
                    (k::OTHER.to_string(), node(other.node)),
                    (k::SENSOR.to_string(), Value::Bool(touch.sensor)),
                    (k::REMOVED.to_string(), Value::Bool(touch.removed)),
                ];
                if start {
                    let side = |(p1, p2, n): &([f32; $N], [f32; $N], [f32; $N])| {
                        if first {
                            (*p1, *n)
                        } else {
                            (*p2, n.map(|x| -x))
                        }
                    };
                    let (points, normals): (Vec<Value>, Vec<Value>) = touch
                        .contacts
                        .iter()
                        .map(|c| {
                            let (p, n) = side(c);
                            (Value::$towards(p), Value::$towards(n))
                        })
                        .unzip();
                    out.push((k::POINTS.to_string(), Value::List(points)));
                    out.push((k::NORMALS.to_string(), Value::List(normals)));
                }
                Value::Map(out)
            };
            match event {
                Event::Started(a, b, touch) => {
                    tell(*a, hook::COLLISION_ENTER, collision(*b, touch, true, true));
                    tell(*b, hook::COLLISION_ENTER, collision(*a, touch, false, true));
                }
                Event::Stopped(a, b, touch) => {
                    tell(*a, hook::COLLISION_EXIT, collision(*b, touch, true, false));
                    tell(*b, hook::COLLISION_EXIT, collision(*a, touch, false, false));
                }
                Event::Force(a, b, push) => {
                    // rapier's direction runs from the first collider to the
                    // second; the second hears it turned round.
                    let contact = |other: Owner, first: bool| {
                        let turn = |v: [f32; $N]| if first { v } else { v.map(|x| -x) };
                        crate::vocabulary::map([
                            (k::OTHER, node(other.node)),
                            (k::FORCE, Value::Num(f64::from(push.magnitude))),
                            (k::DIRECTION, Value::$towards(turn(push.direction))),
                            (k::TOTAL_FORCE, Value::$towards(turn(push.total))),
                            (k::MAX_FORCE, Value::Num(f64::from(push.max))),
                            (k::STARTED, Value::Bool(push.started)),
                        ])
                    };
                    tell(*a, hook::CONTACT_FORCE, contact(*b, true));
                    tell(*b, hook::CONTACT_FORCE, contact(*a, false));
                }
                Event::Tear(a, tear) => {
                    let record = {
                        let state = eng.resource::<$State>();
                        let state = state.borrow();
                        $tear_record(&state, tear)
                    };
                    balaur_core::events::announce(eng, *a, hook::TEAR, record);
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

        /// The mid-step rules, reading the [`Surfaces`] table rather than
        /// calling a script.
        ///
        /// A hook runs on rapier's own threads, which is why it may not touch
        /// the `Engine`: that is what keeps `unsync-callbacks` off and the
        /// solver threaded.
        pub(crate) struct Hooks<'a> {
            pub(crate) surfaces: &'a Surfaces,
        }

        impl PhysicsHooks for Hooks<'_> {
            fn modify_solver_contacts(&self, context: &mut ContactModificationContext<'_>) {
                let first = self.surfaces.get(&context.collider1).copied();
                let second = self.surfaces.get(&context.collider2).copied();
                // rapier owns the platform maths; the table owns the axis.
                if let Some((axis, angle)) = one_way_axis(context, first, second) {
                    context.update_as_oneway_platform(axis, angle);
                }
                let slide = surface_velocity(context, first, second);
                if slide != <$normal>::ZERO
                    && let Some(rigid) = context.rigid_mut()
                {
                    for contact in rigid.solver_contacts.iter_mut() {
                        contact.tangent_velocity += slide;
                    }
                }
            }
        }

        /// The way a one-way platform lets bodies through from, in the first
        /// collider's frame, whichever of the pair is the platform.
        fn one_way_axis(
            context: &ContactModificationContext<'_>,
            first: Option<Surface>,
            second: Option<Surface>,
        ) -> Option<($normal, crate::scalar::Real)> {
            if let Some(platform) = first.and_then(|s| s.one_way) {
                return Some(platform);
            }
            let (axis, angle) = second.and_then(|s| s.one_way)?;
            // rapier reads the axis in the first collider's frame, so the
            // other's turns into that frame and reverses.
            let a = context.colliders.get(context.collider1)?;
            let b = context.colliders.get(context.collider2)?;
            let world = b.position().rotation * axis;
            Some((-(a.position().rotation.inverse() * world), angle))
        }

        /// The tangent velocity rapier holds the pair to, in world space: the
        /// second collider slides against the first at the first's surface
        /// velocity less the second's.
        fn surface_velocity(
            context: &ContactModificationContext<'_>,
            first: Option<Surface>,
            second: Option<Surface>,
        ) -> $normal {
            let world = |handle: ColliderHandle, surface: Option<Surface>| {
                let local = surface.map_or(<$normal>::ZERO, |s| s.velocity);
                if local == <$normal>::ZERO {
                    return local;
                }
                context
                    .colliders
                    .get(handle)
                    .map_or(<$normal>::ZERO, |c| c.position().rotation * local)
            };
            world(context.collider1, first) - world(context.collider2, second)
        }
    };
}
pub(crate) use functions;

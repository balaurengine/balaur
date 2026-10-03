//! What a script may ask a soft body and do to it, in either dimension: one
//! macro both `softbody3d` and `softbody2d` expand.

/// Particles held, moved and pushed from a script, and what the body's
/// elements carry: the same calls on `softbody3d` and `softbody2d`.
///
/// A node's body is the body it was built as and every piece torn off it:
/// particle, edge and cell indices run over the pieces in [`Family`] order,
/// and a call on the whole body reaches every piece.
macro_rules! runtime_api {
    (
        install = $install:ident,
        state = $State:ty,
        rapier = $rapier:ident,
        handle = $Handle:ty,
        joint = $Joint:path,
        stamp = $stamp:ident,
        component = $component:expr,
        dims = $N:literal,
        point = $Point:ty,
        vector = $vector:path,
        value = $value:path,
        array = $array:path
    ) => {
        pub(crate) fn $install(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            m.describe(&[
                ("softbody_particles", &[$component], "()", "How many particles the body has, every piece torn off it included: a generator decides, not the author."),
                ("softbody_position", &[$component], "(index: int)", "Where one particle is, in world space."),
                ("softbody_velocity", &[$component], "(index: int)", "How fast one particle is moving, in world space."),
                ("softbody_particle", &[$component], "(index: int) -> map", "Everything one particle holds: `#{ position, velocity, force, target, rest_position, initial_rest_position, mass, inverse_mass, pinned, damaged, on_surface }`. `force` is what `add_particle_force` keeps on it, `target` where a held particle is going (nil when none), the two rest positions are relative to the rest centre of mass, before and after plastic flow, and `inverse_mass` is 0 while it is held."),
                ("pin_particle", &[$component], "(index: int)", "Hold one particle where it is, which is how a cloth hangs from a hook."),
                ("unpin_particle", &[$component], "(index: int)", "Let a held particle go; it keeps the velocity it had."),
                ("set_particle_target", &[$component], "(index: int, at: vec)", "Move a held particle to `at` over the next step, with the velocity that takes, which is how a cloth is dragged."),
                ("set_particle_position", &[$component], "(index: int, at: vec)", "Put one particle at `at` with no change of velocity."),
                ("set_particle_velocity", &[$component], "(index: int, velocity: vec)", "Set one particle's velocity; a held one keeps moving at it."),
                ("set_particle_damaged", &[$component], "(index: int, damaged: bool)", "Mark one particle as damaged or mend it: a damaged particle tears as an outside one does, which seeds where a crack starts."),
                ("attach_particle", &[$component], "(index: int, body: node)", "Tie one particle to a node's rigid body where it is now: the body and the particle pull on each other."),
                ("detach_particle", &[$component], "(index: int)", "Untie one particle from every body it was attached to; answers whether it was attached."),
                ("softbody_attachments", &[$component], "() -> list", "Every particle tied to a rigid body, as `#{ particle, body, anchor, impulse }`: the anchor in the body's own space, and the impulse the tie pulled with in the last substep."),
                ("add_softbody_force", &[$component], "(force: vec)", "Push every free particle with `force` each step until `reset_softbody_forces`."),
                ("add_particle_force", &[$component], "(index: int, force: vec)", "Push one particle with `force` each step until `reset_softbody_forces`."),
                ("reset_softbody_forces", &[$component], "()", "Take back every force `add_softbody_force` and `add_particle_force` gave the body."),
                ("apply_softbody_impulse", &[$component], "(impulse: vec)", "Change every free particle's velocity by `impulse` at once, as a kick to the whole body."),
                ("apply_particle_impulse", &[$component], "(index: int, impulse: vec)", "Strike one particle."),
                ("apply_softbody_impulse_at", &[$component], "(impulse: vec, point: vec, radius: float)", "Strike the particles within `radius` of `point`, less the further they are; a radius of 0 strikes them all."),
                ("apply_softbody_radial_impulse", &[$component], "(center: vec, magnitude: float, radius: float)", "Push the particles within `radius` away from `center`, as a blast does."),
                ("softbody_edges", &[$component], "()", "Every edge as the two particle indices it joins, in the order `softbody_stress` reports them."),
                ("softbody_stress", &[$component], "()", "Each edge's load as a fraction of its tear threshold, smoothed over `tear_smoothing`: 0 slack, 1 tearing; the larger of its stretch over `tear_strain` and its force over `tear_force`, and 0 while neither is set."),
                ("softbody_edge", &[$component], "(index: int) -> map", "Everything one edge holds: `#{ particles, kind, rest_length, initial_rest_length, plastic_strain, tension_only, softness_hz, softness_damping_ratio, tear_resistance, impulse, stress }`; `kind` is `structural` or `bending`, and the softness is the edge's own or its kind's from the material."),
                ("softbody_cells", &[$component], "()", "Every cell as its particle indices: triangles in 2D, tetrahedra in 3D."),
                ("softbody_cell", &[$component], "(index: int) -> map", "Everything one cell holds: `#{ particles, rest_volume, stiffness_scale, tear_resistance, stress, plastic_stretch }`; `stress` is its strain as a fraction of `tear_strain`, `plastic_stretch` the rows of the permanent stretch of its rest shape."),
                ("softbody_boundary", &[$component], "()", "The body's surface as particle indices: segments in 2D, triangles in 3D."),
                ("softbody_volume_pieces", &[$component], "()", "Each closed piece of the surface whose volume is held, as `#{ particles, rest_volume, volume }`; an open sheet has none."),
                ("softbody_particle_radius", &[$component], "()", "How thick the particles are: what was asked for, or what the layout worked out."),
                ("softbody_mass", &[$component], "()", "What the whole body weighs, every piece included."),
                ("softbody_contacts", &[$component], "() -> map", "What the body's surface touched in the last step: `#{ edges, vertices, volumes }`. Each edge and vertex contact with another soft body or itself is a pair of world points, the two witnesses; each volume contact is `#{ center, normal, volume }`."),
                ("tear_edge", &[$component], "(index: int)", "Tear one edge at the end of the next step."),
                ("tear_cell", &[$component], "(index: int)", "Tear one cell at the end of the next step: a particle near its middle splits across its main stretch."),
                ("has_pending_tears", &[$component], "()", "Whether some edge or cell is marked to tear at the end of the next step."),
                ("set_edge_tear_resistance", &[$component], "(index: int, resistance: float)", "How many times the material's threshold one edge takes to tear; 1 is the material's."),
                ("set_cell_tear_resistance", &[$component], "(index: int, resistance: float)", "The same for one cell's tear strain."),
                ("softbody_crossing", &[$component], "(blade: list) -> map", "The edges and cells a blade meets, `#{ edges, cells }`, without cutting: the blade is a segment's two points in 2D, a triangle's three in 3D."),
                ("cut_softbody", &[$component], "(blade: list) -> list", "Cut the body along a blade at once, as `softbody_crossing` reads it; answers a record for each piece it changed, the one `on_tear` hears."),
                ("tear_softbody", &[$component], "(edges: list, cells: list) -> list", "Tear the given edges and cells at once; answers a record for each piece it changed: `#{ pieces, edges, cells, removed_edges, split_particles, inserted_particles, piece_particles, clusters, moved_joints }`, the one `on_tear` hears. Particle indices are the torn piece's own; `split_particles` holds `[copy, source]` pairs, `clusters` `#{ source, cluster, keeps_proxy }`, and `moved_joints` the joint nodes a cluster split moved."),
                ("reset_plasticity", &[$component], "()", "Undo every permanent set the body took: rest lengths, rest angles and rest shapes go back to how it was built, and it springs back from where it is."),
                ("softbody_sleeping", &[$component], "()", "Whether the body has come to rest and stopped being simulated."),
                ("wake_softbody", &[$component], "()", "Start simulating a resting body again."),
            ]);
            held(m);
            pushed(m);
            read(m);
            elements(m);
            shape(m);
            torn(m);
        }

        // A node whose soft body went away between the lookup and the call.
        fn gone() -> anyhow::Error {
            anyhow::anyhow!("node has no soft body")
        }

        // Run `f` on the node's soft body with the rest of the state beside it.
        fn with<T>(
            eng: &Engine,
            node: NodeId,
            f: impl FnOnce(&mut $State, $Handle) -> anyhow::Result<T>,
        ) -> anyhow::Result<T> {
            let entity = entity_of(node)?;
            let state = eng.resource::<$State>();
            let mut state = state.borrow_mut();
            let handle = *state
                .soft_bodies
                .get(&entity)
                .ok_or_else(|| anyhow::anyhow!("node has no soft body"))?;
            anyhow::ensure!(state.world.soft_bodies.get(handle).is_some(), "node has no soft body");
            f(&mut state, handle)
        }

        type Set = crate::$rapier::dynamics::SoftBodySet;
        type Body = crate::$rapier::prelude::SoftBody;

        // What is counted in each of a body's index spaces.
        const PARTICLES: usize = 0;
        const EDGES: usize = 1;
        const CELLS: usize = 2;

        fn counts(body: &Body) -> [usize; 3] {
            [body.num_particles(), body.edges().len(), body.cells().len()]
        }

        // Every piece of the body, with the family-wide index of its first
        // particle, edge and cell.
        fn pieces(set: &Set, root: $Handle) -> Vec<($Handle, [usize; 3])> {
            use crate::shared::softbody::Family;
            let mut at = [0; 3];
            let mut out = Vec::new();
            for handle in set.family(root) {
                let Some(body) = set.get(handle) else {
                    continue;
                };
                out.push((handle, at));
                let n = counts(body);
                for slot in 0..3 {
                    at[slot] += n[slot];
                }
            }
            out
        }

        // The piece holding the family-wide element `index` of `slot`, and its
        // own index there.
        fn locate(set: &Set, root: $Handle, slot: usize, index: i64) -> anyhow::Result<($Handle, usize)> {
            let what = ["particle", "edge", "cell"][slot];
            let mut left = usize::try_from(index)
                .map_err(|_| anyhow::anyhow!("this body has no {what} {index}"))?;
            for (handle, _) in pieces(set, root) {
                let n = set.get(handle).map_or(0, |body| counts(body)[slot]);
                if left < n {
                    return Ok((handle, left));
                }
                left -= n;
            }
            Err(anyhow::anyhow!("this body has no {what} {index}"))
        }

        // Run `f` on the piece holding element `index` of `slot`.
        fn on_element<T>(
            eng: &Engine,
            node: NodeId,
            slot: usize,
            index: i64,
            f: impl FnOnce(&mut Body, usize) -> anyhow::Result<T>,
        ) -> anyhow::Result<T> {
            with(eng, node, |state, root| {
                let (piece, i) = locate(&state.world.soft_bodies, root, slot, index)?;
                f(state.world.soft_bodies.get_mut(piece).ok_or_else(gone)?, i)
            })
        }

        fn on_every(eng: &Engine, node: NodeId, f: impl Fn(&mut Body)) -> anyhow::Result<()> {
            with(eng, node, |state, root| {
                for (piece, _) in pieces(&state.world.soft_bodies, root) {
                    if let Some(body) = state.world.soft_bodies.get_mut(piece) {
                        f(body);
                    }
                }
                Ok(())
            })
        }

        // A family-wide list read off every piece, its particle indices offset.
        fn gather(
            eng: &Engine,
            node: NodeId,
            f: impl Fn(&Set, &Body, u32) -> Vec<balaur_script::Value>,
        ) -> anyhow::Result<balaur_script::Value> {
            with(eng, node, |state, root| {
                let set = &state.world.soft_bodies;
                let mut out = Vec::new();
                for (piece, at) in pieces(set, root) {
                    if let Some(body) = set.get(piece) {
                        out.extend(f(set, body, at[PARTICLES] as u32));
                    }
                }
                Ok(balaur_script::Value::List(out))
            })
        }

        fn at(value: &balaur_script::Value) -> anyhow::Result<crate::scalar::Real> {
            match value {
                balaur_script::Value::Num(n) => Ok(*n as crate::scalar::Real),
                balaur_script::Value::Int(n) => Ok(*n as crate::scalar::Real),
                _ => anyhow::bail!("expected a number"),
            }
        }

        fn number(x: crate::scalar::Real) -> balaur_script::Value {
            balaur_script::Value::Num(f64::from(crate::scalar::f32_of(x)))
        }

        fn indices<const M: usize>(tuple: [u32; M], offset: u32) -> balaur_script::Value {
            balaur_script::Value::List(
                tuple.iter().map(|i| balaur_script::Value::Int(i64::from(i + offset))).collect(),
            )
        }

        fn table(pairs: Vec<(&str, balaur_script::Value)>) -> balaur_script::Value {
            balaur_script::Value::Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
        }

        fn whole_numbers(value: &balaur_script::Value) -> anyhow::Result<Vec<i64>> {
            let balaur_script::Value::List(items) = value else {
                anyhow::bail!("expected a list of indices");
            };
            items
                .iter()
                .map(|item| match item {
                    balaur_script::Value::Int(i) => Ok(*i),
                    _ => anyhow::bail!("expected a list of indices"),
                })
                .collect()
        }

        // A segment's two points in 2D, a triangle's three in 3D.
        fn blade(value: &balaur_script::Value) -> anyhow::Result<[$Point; $N]> {
            let balaur_script::Value::List(items) = value else {
                anyhow::bail!("a blade is a list of {} points", $N);
            };
            anyhow::ensure!(items.len() == $N, "a blade is a list of {} points", $N);
            let mut out = [<$Point>::ZERO; $N];
            for (slot, item) in out.iter_mut().zip(items) {
                *slot = $vector(crate::shared::softbody::vector_arg::<$N>(item)?);
            }
            Ok(out)
        }

        // Particles held, moved and read one at a time.
        fn held(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            use balaur_core::Engine;
            use balaur_script::{BindingsExt, NodeId, Value};
            use crate::shared::softbody::vector_arg;
            m.function("softbody_particles", |eng: &Engine, node: NodeId| {
                with(eng, node, |state, root| {
                    let set = &state.world.soft_bodies;
                    let total: usize = pieces(set, root)
                        .iter()
                        .filter_map(|(piece, _)| set.get(*piece))
                        .map(Body::num_particles)
                        .sum();
                    Ok(i64::try_from(total).unwrap_or(i64::MAX))
                })
            });
            m.function("softbody_position", |eng: &Engine, (node, index): (NodeId, i64)| {
                on_element(eng, node, PARTICLES, index, |body, i| Ok($value($array(body.particle_position(i)))))
            });
            m.function("softbody_velocity", |eng: &Engine, (node, index): (NodeId, i64)| {
                on_element(eng, node, PARTICLES, index, |body, i| Ok($value($array(body.particle_velocity(i)))))
            });
            m.function("softbody_particle", |eng: &Engine, (node, index): (NodeId, i64)| {
                use crate::vocabulary::keys as k;
                on_element(eng, node, PARTICLES, index, |body, i| {
                    let p = body.particles()[i];
                    let vector = |v| $value($array(v));
                    Ok(table(vec![
                        (k::POSITION, vector(p.position())),
                        (k::VELOCITY, vector(p.velocity())),
                        (k::FORCE, vector(p.force())),
                        (k::TARGET, p.kinematic_target().map_or(Value::Nil, vector)),
                        (k::REST_POSITION, vector(p.rest_position())),
                        (k::INITIAL_REST_POSITION, vector(p.initial_rest_position())),
                        (k::MASS, number(p.mass())),
                        (k::INVERSE_MASS, number(p.inv_mass())),
                        (k::PINNED, Value::Bool(p.is_pinned())),
                        (k::DAMAGED, Value::Bool(p.is_damaged())),
                        (k::ON_SURFACE, Value::Bool(p.is_on_surface())),
                    ]))
                })
            });
            m.function("pin_particle", |eng: &Engine, (node, index): (NodeId, i64)| {
                on_element(eng, node, PARTICLES, index, |body, i| {
                    body.set_particle_pinned(i, true);
                    Ok(())
                })
            });
            m.function("unpin_particle", |eng: &Engine, (node, index): (NodeId, i64)| {
                on_element(eng, node, PARTICLES, index, |body, i| {
                    body.set_particle_pinned(i, false);
                    Ok(())
                })
            });
            m.function(
                "set_particle_target",
                |eng: &Engine, (node, index, target): (NodeId, i64, Value)| {
                    let target = $vector(vector_arg::<$N>(&target)?);
                    on_element(eng, node, PARTICLES, index, |body, i| {
                        anyhow::ensure!(body.particles()[i].is_pinned(), "particle {index} is free: pin it first");
                        body.set_particle_kinematic_target(i, target);
                        Ok(())
                    })
                },
            );
            m.function(
                "set_particle_position",
                |eng: &Engine, (node, index, target): (NodeId, i64, Value)| {
                    let target = $vector(vector_arg::<$N>(&target)?);
                    on_element(eng, node, PARTICLES, index, |body, i| {
                        body.set_particle_position(i, target);
                        Ok(())
                    })
                },
            );
            m.function(
                "set_particle_velocity",
                |eng: &Engine, (node, index, velocity): (NodeId, i64, Value)| {
                    let velocity = $vector(vector_arg::<$N>(&velocity)?);
                    on_element(eng, node, PARTICLES, index, |body, i| {
                        body.set_particle_velocity(i, velocity);
                        Ok(())
                    })
                },
            );
            m.function(
                "set_particle_damaged",
                |eng: &Engine, (node, index, damaged): (NodeId, i64, bool)| {
                    on_element(eng, node, PARTICLES, index, |body, i| {
                        body.set_particle_damaged(i, damaged);
                        Ok(())
                    })
                },
            );
        }

        // A body tied to rigid bodies, and pushed by forces and impulses.
        fn pushed(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            use balaur_core::{Engine, entity_of};
            use balaur_script::{BindingsExt, NodeId, Value};
            use crate::shared::softbody::vector_arg;

            m.function(
                "attach_particle",
                |eng: &Engine, (node, index, other): (NodeId, i64, NodeId)| {
                    let other = entity_of(other)?;
                    with(eng, node, |state, root| {
                        let target = *state
                            .bodies
                            .get(&other)
                            .ok_or_else(|| anyhow::anyhow!("that node has no rigid body to attach to"))?;
                        let (piece, i) = locate(&state.world.soft_bodies, root, PARTICLES, index)?;
                        let world = &mut state.world;
                        let body = world.soft_bodies.get_mut(piece).ok_or_else(gone)?;
                        body.attach_particle(i, target, &world.bodies);
                        Ok(())
                    })
                },
            );
            m.function("detach_particle", |eng: &Engine, (node, index): (NodeId, i64)| {
                on_element(eng, node, PARTICLES, index, |body, i| Ok(body.detach_particle(i)))
            });
            m.function("softbody_attachments", |eng: &Engine, node: NodeId| {
                use crate::vocabulary::keys as k;
                let owners: Vec<(crate::$rapier::prelude::RigidBodyHandle, u64)> = {
                    let state = eng.resource::<$State>();
                    let state = state.borrow();
                    state.bodies.iter().map(|(e, h)| (*h, e.to_bits().get())).collect()
                };
                gather(eng, node, |_, body, offset| {
                    body.particle_attachments()
                        .iter()
                        .map(|tie| {
                            let owner = owners
                                .iter()
                                .find(|(handle, _)| *handle == tie.body)
                                .map_or(Value::Nil, |(_, bits)| Value::Node(*bits));
                            table(vec![
                                (k::PARTICLE, Value::Int(i64::from(tie.particle + offset))),
                                (k::BODY, owner),
                                (k::ANCHOR, $value($array(tie.local_anchor))),
                                (k::IMPULSE, $value($array(tie.impulse()))),
                            ])
                        })
                        .collect()
                })
            });
            m.function("add_softbody_force", |eng: &Engine, (node, force): (NodeId, Value)| {
                let force = $vector(vector_arg::<$N>(&force)?);
                on_every(eng, node, |body| body.add_force(force, true))
            });
            m.function(
                "add_particle_force",
                |eng: &Engine, (node, index, force): (NodeId, i64, Value)| {
                    let force = $vector(vector_arg::<$N>(&force)?);
                    on_element(eng, node, PARTICLES, index, |body, i| {
                        body.add_particle_force(i, force, true);
                        Ok(())
                    })
                },
            );
            m.function("reset_softbody_forces", |eng: &Engine, node: NodeId| {
                on_every(eng, node, |body| body.reset_forces(true))
            });
            m.function(
                "apply_softbody_impulse",
                |eng: &Engine, (node, impulse): (NodeId, Value)| {
                    let impulse = $vector(vector_arg::<$N>(&impulse)?);
                    on_every(eng, node, |body| body.apply_impulse(impulse, true))
                },
            );
            m.function(
                "apply_particle_impulse",
                |eng: &Engine, (node, index, impulse): (NodeId, i64, Value)| {
                    let impulse = $vector(vector_arg::<$N>(&impulse)?);
                    on_element(eng, node, PARTICLES, index, |body, i| {
                        body.apply_particle_impulse(i, impulse, true);
                        Ok(())
                    })
                },
            );
            m.function(
                "apply_softbody_impulse_at",
                |eng: &Engine, (node, impulse, point, radius): (NodeId, Value, Value, Value)| {
                    let impulse = $vector(vector_arg::<$N>(&impulse)?);
                    let point = $vector(vector_arg::<$N>(&point)?);
                    let radius = at(&radius)?;
                    on_every(eng, node, |body| body.apply_impulse_at_point(impulse, point, radius, true))
                },
            );
            m.function(
                "apply_softbody_radial_impulse",
                |eng: &Engine, (node, center, magnitude, radius): (NodeId, Value, Value, Value)| {
                    let center = $vector(vector_arg::<$N>(&center)?);
                    let (magnitude, radius) = (at(&magnitude)?, at(&radius)?);
                    on_every(eng, node, |body| body.apply_radial_impulse(center, magnitude, radius, true))
                },
            );
        }

        // What the edges carry, and whether the body rests.
        fn read(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            use balaur_core::Engine;
            use balaur_script::{BindingsExt, NodeId, Value};

            m.function("softbody_edges", |eng: &Engine, node: NodeId| {
                gather(eng, node, |_, body, offset| {
                    body.edges().iter().map(|edge| indices(edge.vertices, offset)).collect()
                })
            });
            m.function("softbody_stress", |eng: &Engine, node: NodeId| {
                gather(eng, node, |_, body, _| {
                    body.edges().iter().map(|edge| Value::Num(f64::from(edge.stress()))).collect()
                })
            });
            m.function("softbody_sleeping", |eng: &Engine, node: NodeId| {
                with(eng, node, |state, root| {
                    let set = &state.world.soft_bodies;
                    Ok(pieces(set, root)
                        .iter()
                        .filter_map(|(piece, _)| set.get(*piece))
                        .all(Body::is_sleeping))
                })
            });
            m.function("wake_softbody", |eng: &Engine, node: NodeId| {
                on_every(eng, node, Body::wake_up)
            });
            m.function("softbody_particle_radius", |eng: &Engine, node: NodeId| {
                with(eng, node, |state, root| {
                    let body = state.world.soft_bodies.get(root).ok_or_else(gone)?;
                    Ok(crate::scalar::f32_of(body.particle_radius()))
                })
            });
            m.function("softbody_mass", |eng: &Engine, node: NodeId| {
                with(eng, node, |state, root| {
                    let set = &state.world.soft_bodies;
                    let mass: crate::scalar::Real = pieces(set, root)
                        .iter()
                        .filter_map(|(piece, _)| set.get(*piece))
                        .map(Body::mass)
                        .sum();
                    Ok(crate::scalar::f32_of(mass))
                })
            });
            m.function("softbody_contacts", |eng: &Engine, node: NodeId| {
                use crate::vocabulary::keys as k;
                with(eng, node, |state, root| {
                    let set = &state.world.soft_bodies;
                    let pair = |(a, b)| Value::List(vec![$value($array(a)), $value($array(b))]);
                    let (mut edges, mut vertices, mut volumes) = (Vec::new(), Vec::new(), Vec::new());
                    for (piece, _) in pieces(set, root) {
                        let Some(body) = set.get(piece) else {
                            continue;
                        };
                        edges.extend(body.edge_contact_segments(set).map(pair));
                        vertices.extend(body.vertex_contact_segments(set).map(pair));
                        volumes.extend(body.volume_contacts().map(|contact| {
                            table(vec![
                                (k::CENTER, $value($array(contact.center))),
                                (k::NORMAL, $value($array(contact.normal))),
                                (k::VOLUME, number(contact.volume)),
                            ])
                        }));
                    }
                    Ok(table(vec![
                        (k::EDGES, Value::List(edges)),
                        (k::VERTICES, Value::List(vertices)),
                        (k::VOLUMES, Value::List(volumes)),
                    ]))
                })
            });
        }

        // One edge, one cell, the boundary and the held volumes.
        fn elements(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            use balaur_core::Engine;
            use balaur_script::{BindingsExt, NodeId, Value};
            use crate::vocabulary::{keys as k, words as w};

            m.function("softbody_edge", |eng: &Engine, (node, index): (NodeId, i64)| {
                with(eng, node, |state, root| {
                    let set = &state.world.soft_bodies;
                    let (piece, i) = locate(set, root, EDGES, index)?;
                    let offset = pieces(set, root)
                        .iter()
                        .find(|(handle, _)| *handle == piece)
                        .map_or(0, |(_, at)| at[PARTICLES] as u32);
                    let body = set.get(piece).ok_or_else(gone)?;
                    let edge = &body.edges()[i];
                    let structural =
                        edge.kind == crate::$rapier::dynamics::SoftBodyEdgeKind::Structural;
                    let material = body.material();
                    let spring = edge.softness.unwrap_or(if structural {
                        material.edge_softness
                    } else {
                        material.bend_softness
                    });
                    let kind = if structural { w::STRUCTURAL } else { w::BENDING };
                    Ok(table(vec![
                        (k::PARTICLES, indices(edge.vertices, offset)),
                        (k::KIND, Value::Str(kind.into())),
                        (k::REST_LENGTH, number(edge.rest_length)),
                        (k::INITIAL_REST_LENGTH, number(edge.initial_rest_length())),
                        (k::PLASTIC_STRAIN, number(edge.plastic_strain())),
                        (k::TENSION_ONLY, Value::Bool(edge.tension_only)),
                        (k::SOFTNESS_HZ, number(spring.natural_frequency)),
                        (k::SOFTNESS_DAMPING_RATIO, number(spring.damping_ratio)),
                        (k::TEAR_RESISTANCE, number(edge.tear_resistance)),
                        (k::IMPULSE, number(edge.impulse())),
                        (k::STRESS, number(edge.stress())),
                    ]))
                })
            });
        }

        // The boundary, the cells, the held volumes and the tear thresholds.
        fn shape(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            use balaur_core::Engine;
            use balaur_script::{BindingsExt, NodeId, Value};
            use crate::vocabulary::keys as k;

            m.function("softbody_cells", |eng: &Engine, node: NodeId| {
                gather(eng, node, |_, body, offset| {
                    body.cells().iter().map(|cell| indices(cell.vertices, offset)).collect()
                })
            });
            m.function("softbody_cell", |eng: &Engine, (node, index): (NodeId, i64)| {
                with(eng, node, |state, root| {
                    let set = &state.world.soft_bodies;
                    let (piece, i) = locate(set, root, CELLS, index)?;
                    let offset = pieces(set, root)
                        .iter()
                        .find(|(handle, _)| *handle == piece)
                        .map_or(0, |(_, at)| at[PARTICLES] as u32);
                    let body = set.get(piece).ok_or_else(gone)?;
                    let cell = &body.cells()[i];
                    let stretch = cell.plastic_stretch();
                    let rows = (0..$N).map(|r| $value($array(stretch.row(r)))).collect();
                    Ok(table(vec![
                        (k::PARTICLES, indices(cell.vertices, offset)),
                        (k::REST_VOLUME, number(cell.rest_volume)),
                        (k::STIFFNESS_SCALE, number(cell.stiffness_scale)),
                        (k::TEAR_RESISTANCE, number(cell.tear_resistance)),
                        (k::STRESS, number(cell.stress())),
                        (k::PLASTIC_STRETCH, Value::List(rows)),
                    ]))
                })
            });
            m.function("softbody_boundary", |eng: &Engine, node: NodeId| {
                gather(eng, node, |_, body, offset| {
                    body.boundary().iter().map(|element| indices(*element, offset)).collect()
                })
            });
            m.function("softbody_volume_pieces", |eng: &Engine, node: NodeId| {
                gather(eng, node, |_, body, offset| {
                    body.volume_pieces()
                        .iter()
                        .map(|piece| {
                            let particles = piece
                                .particles()
                                .iter()
                                .map(|p| Value::Int(i64::from(p + offset)))
                                .collect();
                            table(vec![
                                (k::PARTICLES, Value::List(particles)),
                                (k::REST_VOLUME, number(piece.rest_volume())),
                                (k::VOLUME, number(piece.volume(body))),
                            ])
                        })
                        .collect()
                })
            });
            m.function(
                "set_edge_tear_resistance",
                |eng: &Engine, (node, index, resistance): (NodeId, i64, f32)| {
                    on_element(eng, node, EDGES, index, |body, i| {
                        body.set_edge_tear_resistance(i, crate::scalar::real(resistance));
                        Ok(())
                    })
                },
            );
            m.function(
                "set_cell_tear_resistance",
                |eng: &Engine, (node, index, resistance): (NodeId, i64, f32)| {
                    on_element(eng, node, CELLS, index, |body, i| {
                        body.set_cell_tear_resistance(i, crate::scalar::real(resistance));
                        Ok(())
                    })
                },
            );
            m.function("reset_plasticity", |eng: &Engine, node: NodeId| {
                on_every(eng, node, Body::reset_plasticity)
            });
        }

        // Tearing and cutting, marked for the next step or done at once.
        fn torn(m: &mut dyn balaur_script::Bindings<balaur_core::Engine>) {
            use balaur_core::Engine;
            use balaur_script::{BindingsExt, NodeId, Value};
            use crate::vocabulary::keys as k;

            m.function("tear_edge", |eng: &Engine, (node, index): (NodeId, i64)| {
                on_element(eng, node, EDGES, index, |body, i| {
                    body.tear_edge(i);
                    Ok(())
                })
            });
            m.function("tear_cell", |eng: &Engine, (node, index): (NodeId, i64)| {
                on_element(eng, node, CELLS, index, |body, i| {
                    body.tear_cell(i);
                    Ok(())
                })
            });
            m.function("has_pending_tears", |eng: &Engine, node: NodeId| {
                with(eng, node, |state, root| {
                    let set = &state.world.soft_bodies;
                    Ok(pieces(set, root)
                        .iter()
                        .filter_map(|(piece, _)| set.get(*piece))
                        .any(Body::has_pending_tears))
                })
            });
            m.function("softbody_crossing", |eng: &Engine, (node, edge): (NodeId, Value)| {
                let blade = blade(&edge)?;
                with(eng, node, |state, root| {
                    let set = &state.world.soft_bodies;
                    let (mut edges, mut cells) = (Vec::new(), Vec::new());
                    for (piece, at) in pieces(set, root) {
                        let Some(body) = set.get(piece) else {
                            continue;
                        };
                        let (e, c) = body.crossing_elements(&blade);
                        let shift = |list: Vec<u32>, from: usize| {
                            list.into_iter().map(move |i| Value::Int(i64::try_from(i as usize + from).unwrap_or(i64::MAX)))
                        };
                        edges.extend(shift(e, at[EDGES]));
                        cells.extend(shift(c, at[CELLS]));
                    }
                    Ok(table(vec![(k::EDGES, Value::List(edges)), (k::CELLS, Value::List(cells))]))
                })
            });
            m.function("cut_softbody", |eng: &Engine, (node, edge): (NodeId, Value)| {
                let blade = blade(&edge)?;
                let entity = balaur_core::entity_of(node)?;
                with(eng, node, |state, root| {
                    let mut out = Vec::new();
                    for (piece, _) in pieces(&state.world.soft_bodies, root) {
                        if let Some(event) = state.world.cut_soft_body(piece, &blade) {
                            out.push(tear_record(state, &event));
                        }
                    }
                    $stamp(&mut state.world, root, entity);
                    Ok(Value::List(out))
                })
            });
            m.function(
                "tear_softbody",
                |eng: &Engine, (node, edges, cells): (NodeId, Value, Value)| {
                    let (edges, cells) = (whole_numbers(&edges)?, whole_numbers(&cells)?);
                    let entity = balaur_core::entity_of(node)?;
                    with(eng, node, |state, root| {
                        let set = &state.world.soft_bodies;
                        let mut wanted: Vec<($Handle, Vec<u32>, Vec<u32>)> = Vec::new();
                        for (slot, list) in [(EDGES, &edges), (CELLS, &cells)] {
                            for index in list {
                                let (piece, i) = locate(set, root, slot, *index)?;
                                let at = match wanted.iter().position(|(h, _, _)| *h == piece) {
                                    Some(at) => at,
                                    None => {
                                        wanted.push((piece, Vec::new(), Vec::new()));
                                        wanted.len() - 1
                                    }
                                };
                                let row = &mut wanted[at];
                                if slot == EDGES { row.1.push(i as u32) } else { row.2.push(i as u32) }
                            }
                        }
                        let mut out = Vec::new();
                        for (piece, edges, cells) in wanted {
                            if let Some(event) = state.world.tear_soft_body(piece, &edges, &cells) {
                                out.push(tear_record(state, &event));
                            }
                        }
                        $stamp(&mut state.world, root, entity);
                        Ok(Value::List(out))
                    })
                },
            );
        }

        /// What a tear or a cut did, in the script's terms: how many pieces,
        /// the torn edges and cells, the edges removed with them, the split
        /// and inserted particles, each piece's particles, the regions split
        /// and the joints moved. Particle indices are the torn body's own.
        pub(crate) fn tear_record(
            state: &$State,
            event: &crate::$rapier::dynamics::SoftBodyTearEvent,
        ) -> balaur_script::Value {
            use balaur_script::Value;
            use crate::vocabulary::keys as k;
            let pairs = |list: &[[u32; 2]]| Value::List(list.iter().map(|p| indices(*p, 0)).collect());
            let ints = |list: &[u32]| Value::List(list.iter().map(|i| Value::Int(i64::from(*i))).collect());
            let joint_node = |handle| {
                state
                    .joints
                    .iter()
                    .find(|(_, r)| matches!(r.handle, $Joint(h) if h == handle))
                    .map_or(Value::Nil, |(e, _)| Value::Node(e.to_bits().get()))
            };
            table(vec![
                (k::PIECES, Value::Int(i64::try_from(event.pieces.len()).unwrap_or(i64::MAX))),
                (k::TORN_EDGES, pairs(&event.torn_edges)),
                (k::CELLS, Value::List(event.torn_cells.iter().map(|c| indices(*c, 0)).collect())),
                (k::REMOVED_EDGES, pairs(&event.removed_edges)),
                (
                    k::SPLIT_PARTICLES,
                    Value::List(event.split_particles.iter().map(|(copy, source)| indices([*copy, *source], 0)).collect()),
                ),
                (k::INSERTED_PARTICLES, ints(&event.inserted_particles)),
                (
                    k::PIECE_PARTICLES,
                    Value::List(event.pieces.iter().map(|piece| ints(&piece.particles)).collect()),
                ),
                (
                    k::CLUSTERS,
                    Value::List(
                        event
                            .clusters
                            .iter()
                            .map(|split| {
                                table(vec![
                                    (k::SOURCE, Value::Int(i64::from(split.source_cluster))),
                                    (k::CLUSTER, Value::Int(i64::from(split.cluster))),
                                    (k::KEEPS_PROXY, Value::Bool(split.keeps_proxy)),
                                ])
                            })
                            .collect(),
                    ),
                ),
                (
                    k::MOVED_JOINTS,
                    Value::List(event.moved_joints.iter().map(|moved| joint_node(moved.joint)).collect()),
                ),
            ])
        }
    };
}

pub(crate) use runtime_api;

//! The link cache the 2D and 3D material managers keep, written once.
//!
//! Both dimensions do the same bookkeeping around kiss3d's global material
//! manager: link a `material` reference on first use, remember the ones that
//! would not link so a broken shader costs one log line rather than sixty a
//! second, drop the lot when an asset reload invalidates them, and keep the
//! channel material a debug view draws with. Only the manager, the material
//! trait and the name prefix differ, so `define!` takes those and stamps out
//! the struct and its methods where the module's own imports resolve.

/// Declare a dimension's material cache: the struct, its methods, and the
/// name its materials are registered under.
macro_rules! define {
    (
        cache = $Cache:ident,
        shared = $Shared:ty,
        boxed = $Boxed:ty,
        manager = $Manager:ty,
        prefix = $prefix:literal,
        channel_shader = $channel_shader:expr,
        channel_material = $channel_material:path $(,)?
    ) => {
        /// The materials this run has linked: one pipeline per `material`
        /// reference, however many nodes name it.
        #[derive(Default)]
        pub(crate) struct $Cache {
            /// `None` records a material that would not link, so the error is
            /// logged once rather than every frame.
            linked: std::collections::HashMap<String, Option<$Shared>>,
            /// The asset generation these were linked at.
            generation: u64,
            /// The channel material and which channel it draws.
            channel: Option<(String, $Shared)>,
            /// The channel the last frame drew; a change rebuilds every node.
            active: String,
            /// The previewing material's probe, shared with it so the value
            /// can be read without reaching back through the material.
            probe: Option<std::rc::Rc<Probe>>,
        }

        impl $Cache {
            /// Drop everything linked before the last reload, and say whether
            /// that happened.
            ///
            /// A caller that answers `true` must also rebuild the nodes
            /// drawing with a material: they hold the old pipeline, and
            /// clearing a cache does not reach into a scene graph.
            pub(crate) fn refresh(&mut self, app: &balaur_core::App) -> bool {
                let now = balaur_core::assets::generation(&app.engine);
                if now == self.generation {
                    return false;
                }
                self.generation = now;
                let had = !self.linked.is_empty();
                for reference in self.linked.keys() {
                    <$Manager>::get_global_manager(|manager| {
                        manager.remove(&manager_name(reference));
                    });
                }
                self.linked.clear();
                had
            }

            /// Point the previewing material's probe at a pixel and read back
            /// what the frame before wrote there.
            ///
            /// One frame behind, which is what a pointer hovering a viewport
            /// wants anyway: the read stalls until the GPU catches up, so it
            /// happens once per ask and not once per draw.
            pub(crate) fn answer_probe(&self, app: &balaur_core::App) {
                let Some(at) = crate::debug_view::probe_at(&app.engine) else {
                    return;
                };
                crate::debug_view::publish_probe(&app.engine, self.probe(at));
            }

            fn probe(&self, at: [f32; 2]) -> Option<[f32; 4]> {
                let probe = self.probe.as_ref()?;
                let read = probe.read();
                probe.aim(at);
                read
            }

            /// Whether the channel view changed since the last frame.
            ///
            /// Turning one on or off changes every node, not only those naming
            /// a material, so the caller rebuilds all of them.
            pub(crate) fn channel_changed(&mut self, channel: &str) -> bool {
                if self.active == channel {
                    return false;
                }
                self.active = channel.to_string();
                true
            }

            /// The material a node draws with: the channel while a view is on,
            /// its own otherwise, and none at all — kiss3d's — when it names
            /// neither.
            pub(crate) fn for_node(
                &mut self,
                app: &balaur_core::App,
                reference: &str,
                channel: &str,
            ) -> Option<$Shared> {
                if !channel.is_empty() {
                    return self.channel(channel);
                }
                if reference.is_empty() {
                    return None;
                }
                self.get(app, reference)
            }

            /// The material that draws `channel`, built on first use.
            pub(crate) fn channel(&mut self, channel: &str) -> Option<$Shared> {
                if let Some((drawn, material)) = &self.channel {
                    if drawn == channel {
                        return Some(material.clone());
                    }
                }
                let features: Vec<(&str, bool)> = crate::shaders::CHANNELS
                    .iter()
                    .map(|c| (*c, *c == channel))
                    .collect();
                let built = crate::shaders::link(
                    &[("package::channel", $channel_shader)],
                    "package::channel",
                    &features,
                )
                .map(|unit| {
                    $channel_material(&crate::material::Compiled {
                        wgsl: crate::shaders::wgsl(&unit),
                        fields: Vec::new(),
                        params: Vec::new(),
                        probes: false,
                    })
                })
                .inspect_err(|why| tracing::error!(channel, "{why:#}"))
                .ok()?;
                let shared: $Shared = std::rc::Rc::new(std::cell::RefCell::new(Box::new(built)));
                <$Manager>::get_global_manager(|manager| {
                    manager.add(shared.clone(), concat!($prefix, ":channel"));
                });
                self.channel = Some((channel.to_string(), shared.clone()));
                Some(shared)
            }

            /// The material `reference` names, linking it on first use.
            ///
            /// `None` for one that will not link — the node keeps kiss3d's own
            /// material, so a shader with a typo in it costs a log line and a
            /// plain object rather than the frame.
            pub(crate) fn get(
                &mut self,
                app: &balaur_core::App,
                reference: &str,
            ) -> Option<$Shared> {
                if let Some(hit) = self.linked.get(reference) {
                    return hit.clone();
                }
                let built = build(app, reference)
                    .inspect_err(|why| tracing::error!(material = reference, "{why:#}"))
                    .ok()
                    .map(|(material, probe)| {
                        self.probe = probe;
                        let boxed: $Boxed = Box::new(material);
                        let shared: $Shared = std::rc::Rc::new(std::cell::RefCell::new(boxed));
                        // Registered, not just attached: kiss3d calls
                        // `begin_frame` only over the manager's own materials.
                        <$Manager>::get_global_manager(|manager| {
                            manager.add(shared.clone(), &manager_name(reference));
                        });
                        shared
                    });
                self.linked.insert(reference.to_string(), built.clone());
                built
            }
        }

        /// What a material is registered under in kiss3d's global manager.
        /// Prefixed so a project's path can never collide with kiss3d's own
        /// names.
        fn manager_name(reference: &str) -> String {
            format!(concat!($prefix, ":{}"), reference)
        }
    };
}
pub(crate) use define;

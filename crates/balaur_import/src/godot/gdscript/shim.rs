//! `gd.rn`: Godot's Variant semantics as one Rune module.
//!
//! Godot answers `size()`, `is_empty()` and `get()` on strings, arrays,
//! dictionaries and objects alike, and Rune's types do not. Rather than infer
//! a static type for every expression, the emitter routes those calls here and
//! this dispatches on the value.

/// The text written to `gd.rn` beside a converted project, kept as Rune
/// files so they read and lint as Rune.
pub(crate) const SHIM: &str = concat!(include_str!("gd_values.rn"), include_str!("gd_nodes.rn"));

#[cfg(test)]
mod tests {
    /// Every `(gd.name)` the translator writes names a function the shim
    /// defines: one it lacks compiles, and fails only when a game reaches it.
    #[test]
    fn every_shim_call_the_translator_writes_is_defined() {
        let written = [
            include_str!("map.rs"),
            include_str!("emit.rs"),
            include_str!("emit/calls.rs"),
            include_str!("../script.rs"),
            include_str!("../script/members.rs"),
            include_str!("../resource.rs"),
        ];
        let defined: std::collections::BTreeSet<&str> = super::SHIM
            .lines()
            .filter_map(|line| {
                let rest = line
                    .strip_prefix("pub fn ")
                    .or_else(|| line.strip_prefix("pub async fn "))?;
                rest.split('(').next()
            })
            .collect();
        let mut missing = Vec::new();
        for text in written {
            for piece in text.split("(gd.").skip(1) {
                let name: String = piece
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if piece[name.len()..].starts_with(')') && !defined.contains(name.as_str()) {
                    missing.push(name);
                }
            }
        }
        assert!(missing.is_empty(), "the shim defines none of {missing:?}");
    }

    /// The shim's `widget_handler_key` is a copy of `WIDGET_SIGNALS`, since
    /// Rune cannot read the Rust table; this keeps the two the same.
    #[test]
    fn the_shim_hears_every_widget_signal_the_translator_does() {
        let body = super::SHIM
            .split("fn widget_handler_key(name) {")
            .nth(1)
            .and_then(|rest| rest.split("_ => (),").next())
            .expect("the shim has widget_handler_key");
        let mut arms: Vec<(String, String)> = body
            .lines()
            .filter_map(|line| {
                let (signal, key) = line.trim().trim_end_matches(',').split_once(" => ")?;
                Some((
                    signal.trim_matches('"').to_string(),
                    key.trim_matches('"').to_string(),
                ))
            })
            .collect();
        let mut table: Vec<(String, String)> = crate::godot::gdscript::map::widgets::WIDGET_SIGNALS
            .iter()
            .map(|(signal, key)| ((*signal).to_string(), (*key).to_string()))
            .collect();
        arms.sort();
        table.sort();
        assert_eq!(arms, table);
    }

    /// The records the translator writes are read by the shim under the
    /// same keys.
    #[test]
    fn the_shim_reads_the_record_keys_the_translator_writes() {
        use crate::godot::gdscript::map;
        for (name, key) in [
            ("BOUND_OWNER", map::BOUND_OWNER),
            ("BOUND_METHOD", map::BOUND_METHOD),
            ("BOUND_ARGS", map::BOUND_ARGS),
            ("CALL_KEY", map::CALL_KEY),
            ("CALL_TAKES", map::CALL_TAKES),
        ] {
            let line = format!("const {name} = \"{key}\";");
            assert!(super::SHIM.contains(&line), "the shim has no `{line}`");
        }
    }

    /// The shim is Rune text inside Rust: nothing but running it says it
    /// still compiles, and every converted script requires it.
    #[test]
    fn the_shim_compiles_and_its_vectors_add() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        put(
            "main.toml",
            "[[nodes]]\nid = \"probe\"\nname = \"Probe\"\nscript = { source = \"probe.rn\" }\n",
        );
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            "pub async fn init(this) {\n\
             \x20   let gd = script::require(\"gd.rn\");\n\
             \x20   let v = (gd.vec2)(1.0, 2.0) + (gd.neg)((gd.vec2)(0.5, 0.5));\n\
             \x20   let waited = (gd.invoke_async1)(#{ \"f\": async |x| x + 1 }, \"f\", 1).await;\n\
             \x20   let wider = (gd.invoke1)((gd.vec2)(1.0, 5.0), \"max\", (gd.vec2)(3.0, 2.0));\n\
             \x20   if (gd.field)(v, \"x\") == 0.5 && (gd.color_of)(\"ff0000\").r == 1.0 && waited == 2\n\
             \x20       && (gd.find)(\"a=b\", \"=\") == 1 && (gd.size)([1, 2]) - 3 == -1\n\
             \x20       && (gd.field)(wider, \"x\") == 3.0 && (gd.field)(wider, \"y\") == 5.0 {\n\
             \x20       this.node.set_visible(false);\n\
             \x20   }\n\
             }\n",
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .visible,
            "the probe hid itself only if the shim compiled and did the sums"
        );
    }

    /// Godot's `AnimationPlayer` verbs reach the `animation` component: a clip
    /// the library holds is found and plays, and one it does not is not found.
    #[test]
    fn an_animation_player_s_verbs_drive_the_animation_component() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        std::fs::create_dir_all(dir.path().join("animations")).unwrap();
        put(
            "animations/steal.toml",
            &[
                "type = \"animation_library\"",
                "[clips.start]",
                "length = 2.0",
                "[[clips.start.tracks]]",
                "target = \"\"",
                "property = \"position\"",
                "interpolation = \"linear\"",
                "keys = [{ time = 0.0, value = [0.0, 0.0, 0.0] }, { time = 2.0, value = [1.0, 0.0, 0.0] }]",
                "",
            ]
            .join("\n"),
        );
        put(
            "main.toml",
            &[
                "[[nodes]]",
                "id = \"probe\"",
                "name = \"Probe\"",
                "script = { source = \"probe.rn\" }",
                "animation = { library = \"animations/steal.toml\" }",
                "",
            ]
            .join("\n"),
        );
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            &[
                "pub fn init(this) {",
                "    let gd = script::require(\"gd.rn\");",
                "    let has = (gd.invoke1)(this.node, \"has_animation\", \"start\");",
                "    let lacks = (gd.invoke1)(this.node, \"has_animation\", \"end\");",
                "    (gd.invoke1)(this.node, \"play\", \"start\");",
                "    (gd.set_field)(this.node, \"speed_scale\", 4.0);",
                "    let playing = (gd.invoke)(this.node, \"is_playing\");",
                "    let current = (gd.invoke)(this.node, \"get_current_animation\");",
                "    let speed = (gd.field)(this.node, \"speed_scale\");",
                "    if has && !lacks && playing && current == \"start\" && speed == 4.0 {",
                "        this.node.set_z_index(7);",
                "    }",
                "}",
                "",
            ]
            .join("\n"),
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert_eq!(
            world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .z_index,
            7,
            "the probe marked itself only if every verb answered as Godot's player"
        );
    }

    /// A multimesh is its node's listed cloner over one `polygon` child: each
    /// instance a copy, placed from a transform a script built axis by axis.
    #[test]
    fn a_multimesh_instance_is_a_listed_copy_where_its_transform_puts_it() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        put(
            "main.toml",
            "[[nodes]]\nid = \"probe\"\nname = \"Probe\"\nscript = { source = \"probe.rn\" }\n",
        );
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            &[
                "pub fn init(this) {",
                "    let gd = script::require(\"gd.rn\");",
                "    let mesh = (gd.array_mesh)();",
                "    let tri = [(gd.vec3)(0.0, 0.0, 0.0), (gd.vec3)(100.0, 0.0, 0.0), (gd.vec3)(0.0, 100.0, 0.0)];",
                "    (mesh[\"add_surface_from_arrays\"])(3, [tri]);",
                "    let mm = (gd.multimesh)();",
                "    (gd.set_field)(mm, \"mesh\", mesh);",
                "    (gd.set_field)(this.node, \"multimesh\", mm);",
                "    (gd.set_field)(mm, \"instance_count\", 2);",
                "    let t = (gd.transform2d)([]);",
                "    t = (gd.with_field)(t, \"x\", (gd.vec2)(2.0, 0.0));",
                "    t = (gd.with_field)(t, \"origin\", (gd.vec2)(300.0, 0.0));",
                "    (mm[\"set_instance_transform_2d\"])(1, t);",
                "    (mm[\"set_instance_color\"])(1, (gd.color)(1.0, 0.0, 0.0, 0.5));",
                "    let copies = this.node.get_component(\"cloner\")[\"copies\"];",
                "    let copy = copies[1];",
                "    let polygon = this.node.get_node(\"mesh\").has_component(\"polygon\");",
                "    if polygon && copies.len() == 2 && copy[\"position\"][0] == 3.0 && copy[\"scale\"][0] == 2.0 && copy[\"tint\"][3] == 0.5 {",
                "        this.node.set_visible(false);",
                "    }",
                "}",
                "",
            ]
            .join("\n"),
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .visible,
            "the probe hid itself only if copy 1 sits at x 3, scaled 2, half see-through"
        );
    }

    /// A node's `_draw` lands on the node's own draw layer, and a texture
    /// region is the part of the picture drawn.
    #[test]
    fn a_node_s_drawing_sits_on_its_layer_and_a_region_cuts_its_picture() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        put(
            "main.toml",
            "[[nodes]]\nid = \"probe\"\nname = \"Probe\"\nz_index = 3\nscript = { source = \"probe.rn\" }\n",
        );
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            &[
                "pub fn _draw(this) {",
                "    let gd = script::require(\"gd.rn\");",
                "    (gd.draw_circle)(this.node, (gd.vec2)(0.0, 0.0), 4.0, (), (), (), ());",
                "    (gd.draw_texture_rect_region)(this.node, \"sheet.png\", (gd.rect)(0.0, 0.0, 8.0, 8.0), (gd.rect)(16.0, 0.0, 16.0, 8.0), (), (), ());",
                "}",
                "pub fn init(this) {",
                "    (script::require(\"gd.rn\").draw_frame)(this.node);",
                "}",
                "",
            ]
            .join("\n"),
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        let shapes = app
            .engine
            .resource::<balaur::render::DrawBuffer2d>()
            .borrow()
            .shapes
            .clone();
        let layers: Vec<Option<i32>> = shapes.iter().map(|d| d.z_index).collect();
        assert_eq!(layers, [Some(3), Some(3)], "{shapes:?}");
        assert!(
            matches!(
                &shapes[1].shape,
                balaur::render::Draw2d::Texture {
                    region: Some([16.0, 0.0, 16.0, 8.0]),
                    ..
                }
            ),
            "{shapes:?}"
        );
    }

    /// A particle count is the engine's rate over one lifetime, both ways,
    /// and a dropdown showing none of its items answers index -1.
    #[test]
    fn a_particle_amount_is_its_rate_over_a_lifetime_and_an_unlisted_pick_is_minus_one() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        put(
            "main.toml",
            &[
                "[[nodes]]",
                "id = \"probe\"",
                "name = \"Probe\"",
                "script = { source = \"probe.rn\" }",
                "particles = { rate = 20.0, lifetime = 2.0 }",
                "",
                "[[nodes]]",
                "id = \"pick\"",
                "name = \"Pick\"",
                "parent = \"probe\"",
                "widget = { kind = \"dropdown\", options = [\"en\", \"ro\"], text = \"fr\" }",
                "",
            ]
            .join("\n"),
        );
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            &[
                "pub fn init(this) {",
                "    let gd = script::require(\"gd.rn\");",
                "    let before = (gd.field)(this.node, \"amount\");",
                "    (gd.set_field)(this.node, \"amount\", 10);",
                "    let rate = this.node.get_component(\"particles\")[\"rate\"];",
                "    let pick = (gd.option_index)(this.node.get_node(\"Pick\"));",
                "    if before == 40 && rate == 5.0 && pick == -1 {",
                "        this.node.set_visible(false);",
                "    }",
                "}",
                "",
            ]
            .join("\n"),
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .visible,
            "the probe hid itself only if amount read 40, wrote a rate of 5, and the pick read -1"
        );
    }

    /// A method bound with arguments takes the signal's values first and
    /// what `bind` fixed after them, as Godot's `Callable.bind` does.
    #[test]
    fn a_bound_method_takes_the_signal_s_values_then_the_bound_ones() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        put(
            "main.toml",
            "[[nodes]]\nid = \"probe\"\nname = \"Probe\"\nscript = { source = \"probe.rn\" }\n",
        );
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            &[
                "pub fn take(this, first, second) {",
                "    if first == 1 && second == 2 {",
                "        this.node.set_visible(false);",
                "    }",
                "}",
                "pub fn init(this) {",
                "    let gd = script::require(\"gd.rn\");",
                "    let bound = #{ \"__bound\": this.node, \"__method\": \"take\", \"__args\": [2] };",
                "    (gd.call_value)(bound, [1]);",
                "}",
                "",
            ]
            .join("\n"),
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .visible,
            "the probe hid itself only if take heard 1 then 2"
        );
    }

    /// Godot's root viewport is the window: its `size` is the window's, which
    /// a game divides to pick its UI scale.
    #[test]
    fn the_root_viewport_s_size_is_the_window_s() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n\n[window]\nwidth = 840\nheight = 1920\n",
        );
        put(
            "main.toml",
            "[[nodes]]\nid = \"probe\"\nname = \"Probe\"\nscript = { source = \"probe.rn\" }\n",
        );
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            &[
                "pub fn init(this) {",
                "    let gd = script::require(\"gd.rn\");",
                "    let size = (gd.field)(scene::root(), \"size\");",
                "    if (gd.field)(size, \"x\") == 840.0 && (gd.field)(size, \"y\") == 1920.0 {",
                "        this.node.set_visible(false);",
                "    }",
                "}",
                "",
            ]
            .join("\n"),
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .visible,
            "the probe hid itself only if the root viewport measured 840 by 1920"
        );
    }

    /// A scene a script names by its Godot path exists when the scene the
    /// import wrote does, so a path built at run time finds it.
    #[test]
    fn a_godot_scene_path_exists_when_its_converted_scene_does() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        put(
            "main.toml",
            "[[nodes]]\nid = \"probe\"\nname = \"Probe\"\nscript = { source = \"probe.rn\" }\n",
        );
        put("ro.toml", "[[nodes]]\nid = \"ro\"\nname = \"Ro\"\n");
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            &[
                "pub fn init(this) {",
                "    let gd = script::require(\"gd.rn\");",
                "    let found = (gd.resource_exists)(`res://${\"ro\"}.tscn`);",
                "    let missing = (gd.resource_exists)(\"res://hu.tscn\");",
                "    if found && !missing {",
                "        this.node.set_visible(false);",
                "    }",
                "}",
                "",
            ]
            .join("\n"),
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .visible,
            "the probe hid itself only if ro.tscn was found as ro.toml and hu.tscn was not"
        );
    }

    /// A `Line2D` is a `shape2d` polyline: its point verbs read and write the
    /// polyline's points in Godot's pixels, y down.
    #[test]
    fn a_line_s_point_verbs_edit_the_polyline() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        put(
            "main.toml",
            &[
                "[[nodes]]",
                "id = \"probe\"",
                "name = \"Probe\"",
                "script = { source = \"probe.rn\" }",
                "",
                "[[nodes]]",
                "id = \"route\"",
                "name = \"Route\"",
                "parent = \"probe\"",
                "shape2d = { kind = \"polyline\", width = 0.1, mesh = { type = \"path2d\", points = [[0.0, 0.0], [1.0, -2.0]] } }",
                "",
            ]
            .join("\n"),
        );
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            &[
                "pub fn init(this) {",
                "    let gd = script::require(\"gd.rn\");",
                "    let line = this.node.get_node(\"Route\");",
                "    let before = (gd.invoke)(line, \"get_point_count\");",
                "    (gd.invoke1)(line, \"add_point\", (gd.vec2)(300.0, 0.0));",
                "    let second = (gd.invoke1)(line, \"get_point_position\", 1);",
                "    let width = (gd.field)(line, \"width\");",
                "    let after = (gd.size)((gd.field)(line, \"points\"));",
                "    if before == 2 && after == 3 && second.x == 100.0 && second.y == 200.0 && width > 9.999 && width < 10.001 {",
                "        this.node.set_visible(false);",
                "    }",
                "}",
                "",
            ]
            .join("\n"),
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .visible,
            "the probe hid itself only if the line counted, grew and read back in pixels"
        );
    }

    /// A node made from a packed scene waits outside the tree until it is
    /// added, so `is_inside_tree` is false until then.
    #[test]
    fn an_instantiated_scene_is_inside_the_tree_only_once_added() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        put(
            "main.toml",
            "[[nodes]]\nid = \"probe\"\nname = \"Probe\"\nscript = { source = \"probe.rn\" }\n",
        );
        put("part.toml", "[[nodes]]\nid = \"part\"\nname = \"Part\"\n");
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            &[
                "pub fn init(this) {",
                "    let gd = script::require(\"gd.rn\");",
                "    let made = (gd.instantiate)((gd.load)(\"res://part.tscn\"));",
                "    let before = (gd.inside_tree)(made);",
                "    let _ = (gd.add_child)(this.node, made);",
                "    let after = (gd.inside_tree)(made);",
                "    if made.is_valid() && !before && after {",
                "        this.node.set_visible(false);",
                "    }",
                "}",
                "",
            ]
            .join("\n"),
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .visible,
            "the probe hid itself only if the part was outside the tree, then inside"
        );
    }

    /// A label in the world keeps its caption on `text2d`: `text` reads and
    /// writes it there, and a widget's stays on the widget.
    #[test]
    fn a_world_label_s_text_is_its_text2d() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        put(
            "main.toml",
            &[
                "[[nodes]]",
                "id = \"probe\"",
                "name = \"Probe\"",
                "script = { source = \"probe.rn\" }",
                "",
                "[[nodes]]",
                "id = \"region\"",
                "name = \"Region\"",
                "parent = \"probe\"",
                "text2d = { text = \"ALBA\" }",
                "",
                "[[nodes]]",
                "id = \"caption\"",
                "name = \"Caption\"",
                "parent = \"probe\"",
                "widget = { kind = \"label\", text = \"Hi\" }",
                "",
            ]
            .join("\n"),
        );
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            &[
                "pub fn init(this) {",
                "    let gd = script::require(\"gd.rn\");",
                "    let region = this.node.get_node(\"Region\");",
                "    let caption = this.node.get_node(\"Caption\");",
                "    let before = (gd.text_of)(region);",
                "    (gd.set_text)(region, \"CLUJ\");",
                "    (gd.set_text)(caption, \"Bye\");",
                "    let moved = region.get_component(\"text2d\")[\"text\"];",
                "    if before == \"ALBA\" && moved == \"CLUJ\" && (gd.text_of)(caption) == \"Bye\" && !region.has_component(\"widget\") {",
                "        this.node.set_visible(false);",
                "    }",
                "}",
                "",
            ]
            .join("\n"),
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .visible,
            "the probe hid itself only if the world label's text moved on its text2d"
        );
    }
}

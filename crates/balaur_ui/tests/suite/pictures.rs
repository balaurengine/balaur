//! `ui::picture`: a picture made from bytes a script holds, drawn by the
//! name it answers wherever a file's path is drawn.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::hecs::Entity;

use crate::support::{root_rect, script_text, settle};

/// A PNG `width` by `height`, as the bytes a script spells.
fn png_list(width: u32, height: u32) -> String {
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        width,
        height,
        image::Rgba([30, 90, 200, 255]),
    ))
    .write_with_encoder(image::codecs::png::PngEncoder::new(&mut out))
    .unwrap();
    let bytes: Vec<String> = out.iter().map(u8::to_string).collect();
    format!("[{}]", bytes.join(", "))
}

/// A `Root` running `body` beside two images that name no picture yet.
fn app_with(body: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"root\"\nname = \"Root\"\nscript = { source = \"root.bs\" }\n\n\
         [[nodes]]\nid = \"big\"\nname = \"Big\"\nparent = \"root\"\nwidget = { kind = \"image\", x = 0.0, y = 0.0 }\n\n\
         [[nodes]]\nid = \"small\"\nname = \"Small\"\nparent = \"root\"\nwidget = { kind = \"image\", x = 0.0, y = 200.0 }\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("root.bs"), body).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    (dir, app)
}

fn node(app: &App, path: &str) -> Entity {
    let world = app.engine.world();
    balaur_core::scene::find_node(&world, app.engine.root(), path).expect("the node")
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "a test's clock, for a decode on a worker"
)]
fn a_picture_made_from_bytes_draws_at_its_own_size_until_it_is_freed() {
    let script = format!(
        "pub class Root {{ node: Node, big: str, small: str, freed: str, whole: str }}
impl Root {{
    pub fn init() {{
        let bytes: [int] = {png};
        self.node.call(\"show\", ui::picture_async(bytes, nil));
        let small = ui::picture(bytes, ui::PictureOptions {{ max_side_pixels: 12 }});
        self.small = small.unwrap();
        self.node.get_node(\"Small\").widget_image.image = self.small;
        let whole = ui::picture(bytes, ui::PictureOptions {{}});
        self.whole = if whole == nil {{ \"nil\" }} else {{ \"named\" }};
        let none = ui::picture([1, 2, 3], nil);
        self.freed = if none == nil {{ \"nil\" }} else {{ \"named\" }};
    }}
    pub async fn show(id: int) {{
        let name = await task::wait(id);
        self.big = name.text();
        self.node.get_node(\"Big\").widget_image.image = self.big;
    }}
    pub fn free() {{
        let once = ui::free_picture(self.small);
        let twice = ui::free_picture(self.small);
        self.freed = \"{{once}} {{twice}}\";
    }}
}}
",
        png = png_list(48, 24)
    );
    let (_dir, mut app) = app_with(&script);
    let root = node(&app, "Root");
    let started = std::time::Instant::now();
    while script_text(&app, root, "big")
        .unwrap_or_default()
        .is_empty()
    {
        assert!(started.elapsed().as_secs() < 30, "the picture never came");
        app.tick(1.0 / 60.0);
    }
    let big = script_text(&app, root, "big").unwrap();
    let small = script_text(&app, root, "small").unwrap();
    assert!(
        big.starts_with("picture:") && small.starts_with("picture:") && big != small,
        "{big} {small}"
    );
    assert_eq!(
        script_text(&app, root, "freed").as_deref(),
        Some("nil"),
        "bytes that are no picture"
    );
    assert_eq!(
        script_text(&app, root, "whole").as_deref(),
        Some("named"),
        "options that leave the size out"
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let (drawn_big, drawn_small) = (
        root_rect(&ctx, node(&app, "Root/Big")),
        root_rect(&ctx, node(&app, "Root/Small")),
    );
    assert_eq!(drawn_big.size(), egui::vec2(48.0, 24.0), "{drawn_big:?}");
    assert_eq!(drawn_small.size(), egui::vec2(12.0, 6.0), "{drawn_small:?}");
    let host = app.engine.script_host().expect("a script host");
    host.call_on(balaur::node_id_of(root), "free", &[]);
    assert_eq!(
        script_text(&app, root, "freed").as_deref(),
        Some("true false")
    );
}

//! A colour a scene or a script writes is the pixel a run's picture holds,
//! drawn on a real GPU. A machine with none skips, as kiss3d's GPU tests do.

use balaur::{AppConfig, standard_app};
use balaur_core::Stage;
use balaur_render::{ClearColorConfig, ScreenshotRequest};
use kiss3d::wgpu;

/// The wall fills the view from just right of the centre; left of it is the
/// background, so one frame holds both colours.
const SCENE: &str = r##"
[[nodes]]
id = "n_wall"
name = "Wall"
script = { source = "scripts/back.bs" }

[nodes.transform]
position = [5.1, 0.0, 0.0]

[nodes.shape2d]
color = "#3c4753"
shape = { kind = "rectangle", size = [10.0, 10.0] }
"##;

const WALL: [u8; 3] = [0x3c, 0x47, 0x53];
const BACKGROUND: [f32; 3] = [0.2, 0.4, 0.6];
const SIDE: u32 = 64;

/// Whether a hardware adapter answers. A software rasterizer counts as none:
/// it is not what a player's picture is drawn with.
fn gpu_present() -> bool {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let asked = instance.request_adapter(&wgpu::RequestAdapterOptions::default());
    pollster::block_on(asked)
        .is_ok_and(|adapter| adapter.get_info().device_type != wgpu::DeviceType::Cpu)
}

/// The first frame drawn with the script's background, as the PNG a run writes.
fn shot() -> image::RgbaImage {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "project.toml",
        "[application]\nname = \"film\"\nmain_scene = \"scenes/main.toml\"\n",
    );
    write("scenes/main.toml", SCENE);
    let [r, g, b] = BACKGROUND;
    write(
        "scripts/back.bs",
        &format!(
            "pub class Back {{ node: Node }}\n\
             impl Back {{ fn init() {{ render::set_background(color({r:?}, {g:?}, {b:?}, 1.0)); }} }}\n"
        ),
    );
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let picture = dir.path().join("shot.png");
    let path = picture.clone();
    let mut asked = false;
    app.add_system(Stage::Last, move |eng, _| {
        if asked {
            // The backend drops the request once the frame is written, and a
            // quit asked any sooner would end the run before that frame drew.
            if eng.try_resource::<ScreenshotRequest>().is_none() {
                eng.request_quit();
            }
            return;
        }
        let Some(clear) = eng.try_resource::<ClearColorConfig>() else {
            return;
        };
        let clear = clear.borrow();
        if !clear.changed && crate::same(clear.color, BACKGROUND) {
            balaur_render::request_screenshot(eng, path.clone());
            asked = true;
        }
    });
    balaur_render::kiss3d_backend::run_offscreen(app, "film", SIDE, SIDE).unwrap();
    image::open(&picture).unwrap().to_rgba8()
}

/// One rounding step either way: the film is half floats and the resolve a
/// power, so the last bit is the GPU's.
#[track_caller]
fn assert_pixel(picture: &image::RgbaImage, x: u32, wanted: [u8; 3], what: &str) {
    let got = picture.get_pixel(x, SIDE / 2).0;
    let near = (0..3).all(|c| got[c].abs_diff(wanted[c]) <= 1);
    assert!(near, "{what}: wanted {wanted:?}, the picture holds {got:?}");
}

#[test]
fn an_authored_colour_is_the_pixel_a_shot_holds() {
    if !gpu_present() {
        eprintln!("skipped: no GPU adapter");
        return;
    }
    let picture = shot();
    assert_pixel(&picture, SIDE - 6, WALL, "a shape2d's `#3c4753`");
    let background = BACKGROUND.map(|channel| (channel * 255.0).round() as u8);
    assert_pixel(&picture, 6, background, "the background a script set");
}

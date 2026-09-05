//! `balaur import` over an `.aseprite` file built by hand here — three
//! frames, two layers, three tags and a slice — so nothing binary has to
//! ship with the tests except the copy on disk that proves the builder and
//! the fixture agree.

#![cfg(feature = "aseprite")]

use balaur_render::SpriteSheet;
use balaur_render::aseprite;

const FIXTURE: &str = "tests/fixtures/walk.aseprite";

fn word(v: u16) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn dword(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn short(v: i16) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn long(v: i32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn string(s: &str) -> Vec<u8> {
    let mut out = word(s.len() as u16);
    out.extend_from_slice(s.as_bytes());
    out
}

fn chunk(kind: u16, body: &[u8]) -> Vec<u8> {
    let mut out = dword((6 + body.len()) as u32);
    out.extend(word(kind));
    out.extend_from_slice(body);
    out
}

fn frame(duration_ms: u16, chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut out = dword((16 + body.len()) as u32);
    out.extend(word(0xF1FA));
    out.extend(word(chunks.len() as u16));
    out.extend(word(duration_ms));
    out.extend([0, 0]);
    out.extend(dword(chunks.len() as u32));
    out.extend(body);
    out
}

fn layer(name: &str, visible: bool) -> Vec<u8> {
    let mut body = word(u16::from(visible) | 2);
    body.extend(word(0));
    body.extend(word(0));
    body.extend(word(0));
    body.extend(word(0));
    body.extend(word(0));
    body.push(255);
    body.extend([0, 0, 0]);
    body.extend(string(name));
    chunk(0x2004, &body)
}

/// A raw cel filling the whole `w` x `h` canvas with one colour.
fn cel(layer: u16, w: u16, h: u16, rgba: [u8; 4]) -> Vec<u8> {
    let mut body = word(layer);
    body.extend(short(0));
    body.extend(short(0));
    body.push(255);
    body.extend(word(0));
    body.extend(short(0));
    body.extend([0; 5]);
    body.extend(word(w));
    body.extend(word(h));
    for _ in 0..(usize::from(w) * usize::from(h)) {
        body.extend(rgba);
    }
    chunk(0x2005, &body)
}

fn tags(tags: &[(&str, u16, u16, u8, u16)]) -> Vec<u8> {
    let mut body = word(tags.len() as u16);
    body.extend([0; 8]);
    for (name, from, to, direction, repeat) in tags {
        body.extend(word(*from));
        body.extend(word(*to));
        body.push(*direction);
        body.extend(word(*repeat));
        body.extend([0; 6]);
        body.extend([0, 0, 0]);
        body.push(0);
        body.extend(string(name));
    }
    chunk(0x2018, &body)
}

fn slice(name: &str, keys: &[(u32, i32, i32, u32, u32)], pivot: Option<(i32, i32)>) -> Vec<u8> {
    let mut body = dword(keys.len() as u32);
    body.extend(dword(if pivot.is_some() { 2 } else { 0 }));
    body.extend(dword(0));
    body.extend(string(name));
    for (frame, x, y, w, h) in keys {
        body.extend(dword(*frame));
        body.extend(long(*x));
        body.extend(long(*y));
        body.extend(dword(*w));
        body.extend(dword(*h));
        if let Some((px, py)) = pivot {
            body.extend(long(px));
            body.extend(long(py));
        }
    }
    chunk(0x2022, &body)
}

const W: u16 = 4;
const H: u16 = 2;
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// Three frames of a 4x2 canvas: red, green, blue on the visible `body`
/// layer and white on a hidden `mask` layer; tags `walk` (0-1), `back`
/// (1-2, reverse) and `bounce` (0-2, ping-pong); a `hitbox` slice with a
/// pivot.
fn fixture() -> Vec<u8> {
    let frames = [
        frame(
            100,
            &[
                layer("body", true),
                layer("mask", false),
                cel(0, W, H, RED),
                cel(1, W, H, WHITE),
                tags(&[("walk", 0, 1, 0, 0), ("back", 1, 2, 1, 0), ("bounce", 0, 2, 2, 0)]),
                slice("hitbox", &[(0, 1, 0, 2, 2)], Some((2, 1))),
            ],
        ),
        frame(200, &[cel(0, W, H, GREEN), cel(1, W, H, WHITE)]),
        frame(300, &[cel(0, W, H, BLUE), cel(1, W, H, WHITE)]),
    ];
    let body: Vec<u8> = frames.concat();
    let mut out = dword((128 + body.len()) as u32);
    out.extend(word(0xA5E0));
    out.extend(word(3));
    out.extend(word(W));
    out.extend(word(H));
    out.extend(word(32));
    out.extend(dword(1));
    out.extend(word(100));
    out.extend(dword(0));
    out.extend(dword(0));
    out.push(0);
    out.extend([0; 3]);
    out.extend(word(0));
    out.extend([1, 1]);
    out.extend(short(0));
    out.extend(short(0));
    out.extend(word(16));
    out.extend(word(16));
    out.extend([0; 84]);
    assert_eq!(out.len(), 128);
    out.extend(body);
    out
}

fn import(layers: &[String]) -> aseprite::AsepriteImport {
    aseprite::import(&fixture(), "walk", "art/walk.png", layers).unwrap()
}

fn pixel(png: &[u8], x: u32, y: u32) -> [u8; 4] {
    let image = image::load_from_memory(png).unwrap().to_rgba8();
    image.get_pixel(x, y).0
}

#[test]
fn the_fixture_on_disk_is_what_the_builder_makes() {
    if std::env::var_os("BALAUR_WRITE_FIXTURES").is_some() {
        std::fs::write(FIXTURE, fixture()).unwrap();
    }
    assert_eq!(
        std::fs::read(FIXTURE).unwrap(),
        fixture(),
        "regenerate it with BALAUR_WRITE_FIXTURES=1"
    );
}

#[test]
fn the_page_packs_every_frame_at_canvas_size() {
    let imported = import(&[]);
    assert_eq!((imported.width, imported.height, imported.frames), (8, 4, 3));
    let image = image::load_from_memory(&imported.png).unwrap();
    assert_eq!((image.width(), image.height()), (8, 4));
    assert_eq!(pixel(&imported.png, 0, 0), RED);
    assert_eq!(pixel(&imported.png, 4, 0), GREEN);
    assert_eq!(pixel(&imported.png, 0, 2), BLUE);
    assert_eq!(pixel(&imported.png, 4, 2), [0, 0, 0, 0], "an empty cell is clear");
}

#[test]
fn a_hidden_layer_is_left_out_unless_named() {
    assert_eq!(pixel(&import(&[]).png, 0, 0), RED);
    assert_eq!(pixel(&import(&["mask".to_string()]).png, 0, 0), WHITE);
    let error = aseprite::import(&fixture(), "walk", "art/walk.png", &["hat".to_string()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("hat") && error.contains("body"), "unhelpful: {error}");
}

#[test]
fn the_sheet_names_every_frame_tag_and_slice() {
    let imported = import(&[]);
    let sheet = SpriteSheet::parse(&toml::from_str(&imported.sheet).unwrap()).unwrap();
    assert_eq!(sheet.texture, "art/walk.png");
    assert_eq!(sheet.frames.len(), 3);
    assert_eq!(sheet.frames[1].rect, [4, 0, 4, 2]);
    assert!((sheet.frames[1].duration - 0.2).abs() < 1e-6);
    assert_eq!(sheet.frames[2].rect, [0, 2, 4, 2]);
    let tag = |name: &str| sheet.tags.iter().find(|t| t.name == name).unwrap().clone();
    assert_eq!((tag("walk").from, tag("walk").to, tag("walk").direction.as_str()), (0, 1, "forward"));
    assert_eq!(tag("back").direction, "reverse");
    assert_eq!(tag("bounce").direction, "pingpong");
    assert_eq!(sheet.slices[0].name, "hitbox");
    assert_eq!(sheet.slices[0].rect, [1, 0, 2, 2]);
    assert_eq!(sheet.slices[0].pivot, Some([2, 1]));
}

#[test]
fn each_tag_becomes_a_step_clip_over_its_frames() {
    let imported = import(&[]);
    let clips: toml::Value = toml::from_str(imported.clips.as_deref().unwrap()).unwrap();
    assert_eq!(clips["type"].as_str(), Some("animation_clip"));
    let clip = |name: &str| clips["clips"][name].clone();
    let keys = |name: &str| -> Vec<(f64, f64)> {
        clip(name)["tracks"][0]["keys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| (k["t"].as_float().unwrap(), k["value"].as_float().unwrap()))
            .collect()
    };
    assert_eq!(clip("walk")["loop"].as_str(), Some("loop"));
    assert!((clip("walk")["length"].as_float().unwrap() - 0.3).abs() < 1e-9);
    assert_eq!(clip("walk")["tracks"][0]["property"].as_str(), Some("sprite/frame"));
    assert_eq!(clip("walk")["tracks"][0]["interp"].as_str(), Some("step"));
    assert_eq!(keys("walk"), vec![(0.0, 0.0), (0.1, 1.0)]);
    assert_eq!(keys("back"), vec![(0.0, 2.0), (0.3, 1.0)], "reverse plays the last frame first");
    assert!((clip("back")["length"].as_float().unwrap() - 0.5).abs() < 1e-9);
    assert_eq!(clip("bounce")["loop"].as_str(), Some("pingpong"));
}

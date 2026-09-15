//! What importing a model holds at once.
//!
//! A model's textures are the bulk of it and nothing here decodes one, so an
//! import should hold the file it is reading and nothing else. Collecting them
//! to write afterwards costs the whole model in memory, which is what this
//! measures rather than trusts.
//!
//! Its own test binary, and one test in it: the counter below is the process's,
//! so a second test allocating beside it would be counted too.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Bytes handed out and not yet returned, and the most there has ever been.
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract, passed straight through.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller's contract, passed straight through.
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// How many textures the model names, and how big each one is. Enough of them
/// that holding all is an order of magnitude more than holding one.
const IMAGES: usize = 24;
const IMAGE_BYTES: usize = 512 * 1024;

/// A `.gltf` naming `IMAGES` textures beside itself: one triangle, one
/// material per image, and every image a file of its own.
fn model() -> String {
    // Three positions as a `data:` buffer, so the geometry needs no side file
    // and the only side files are the textures.
    let positions: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    let mut bytes = Vec::new();
    for value in positions {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let images: Vec<String> = (0..IMAGES)
        .map(|i| format!(r#"{{"uri":"texture{i}.png"}}"#))
        .collect();
    let textures: Vec<String> = (0..IMAGES)
        .map(|i| format!(r#"{{"source":{i}}}"#))
        .collect();
    let materials: Vec<String> = (0..IMAGES)
        .map(|i| {
            format!(
                r#"{{"name":"paint{i}","pbrMetallicRoughness":{{"baseColorTexture":{{"index":{i}}}}}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,
"scenes":[{{"nodes":[0]}}],
"nodes":[{{"name":"Tri","mesh":0}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"material":0}}]}}],
"images":[{images}],
"textures":[{textures}],
"materials":[{materials}],
"buffers":[{{"byteLength":{length},"uri":"data:application/octet-stream;base64,{payload}"}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":{length}}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3",
  "min":[0.0,0.0,0.0],"max":[1.0,1.0,0.0]}}]}}"#,
        images = images.join(","),
        textures = textures.join(","),
        materials = materials.join(","),
        length = bytes.len(),
        payload = base64(&bytes),
    )
}

/// Standard base64, for the one buffer the fixture spells inline.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut buffer = [0u8; 3];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let packed = u32::from(buffer[0]) << 16 | u32::from(buffer[1]) << 8 | u32::from(buffer[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                let index = (packed >> (18 - i * 6)) & 0x3f;
                out.push(char::from(ALPHABET[index as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn importing_a_model_holds_one_texture_at_a_time() {
    let source = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("tri.gltf"), model()).unwrap();
    for i in 0..IMAGES {
        std::fs::write(
            source.path().join(format!("texture{i}.png")),
            vec![0u8; IMAGE_BYTES],
        )
        .unwrap();
    }

    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let imported =
        balaur_import::import_file(&source.path().join("tri.gltf"), project.path(), &[]).unwrap();
    let held = PEAK.load(Ordering::Relaxed) - before;

    // Every texture reached the project, so the import did the work measured.
    for i in 0..IMAGES {
        let rel = format!("models/texture{i}.png");
        assert!(imported.files.contains(&rel), "{rel} was not written");
        assert_eq!(
            std::fs::metadata(project.path().join(&rel)).unwrap().len() as usize,
            IMAGE_BYTES
        );
    }

    // The bound is generous: a few textures' worth covers the document, the
    // scene and whatever the allocator rounds up, and is still far below the
    // whole model. Collecting them all held every byte of it.
    let all = IMAGES * IMAGE_BYTES;
    assert!(
        held < all / 4,
        "held {held} bytes importing {all} of textures, so they are being collected rather \
         than written one at a time"
    );
}

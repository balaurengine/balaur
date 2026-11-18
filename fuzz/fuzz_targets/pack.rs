//! A game pack, read from any bytes, and read from one built out of the input
//! (fields split on zero bytes: the manifest, then names and contents dealt
//! to scenes, scripts, assets and the API in turn, then patches over all but
//! the magic). Reading refuses or yields a pack, and a pack that reads writes
//! itself back entry for entry.
#![no_main]

use balaur_core::pack::{MAGIC, Pack};
use balaur_core::pack_tail::Tail;
use libfuzzer_sys::fuzz_target;

fn check(bytes: &[u8]) {
    let Ok(pack) = Pack::decode(bytes) else {
        return;
    };
    let _ = Tail::of(&pack);
    let again = Pack::decode(&pack.encode()).expect("a pack reads back what it wrote");
    assert_eq!(
        pack.entries(),
        again.entries(),
        "a pack changed on the way through"
    );
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fuzz_target!(|data: &[u8]| {
    check(data);
    let mut fields: Vec<&[u8]> = data.split(|&b| b == 0).collect();
    let patches = fields.pop().unwrap_or_default();
    let mut fields = fields.into_iter();
    let mut pack = Pack {
        manifest: text(fields.next().unwrap_or_default()),
        ..Pack::default()
    };
    let mut slot = 0;
    while let Some(name) = fields.next() {
        let content = fields.next().unwrap_or_default();
        match slot % 4 {
            0 => {
                pack.scenes.insert(text(name), text(content));
            }
            1 => {
                pack.scripts.insert(text(name), content.to_vec());
            }
            2 => {
                pack.assets.insert(text(name), content.to_vec());
            }
            _ => pack.api.push(text(name)),
        }
        slot += 1;
    }
    check(&balaur_fuzz::patched(pack.encode(), MAGIC.len(), patches));
});

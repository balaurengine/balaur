//! What a websocket peer sends: a handshake and a close frame from any bytes,
//! and a compressed stream as an extensions line up to the first zero byte,
//! then payloads each led by a little-endian u16 length. Each reads, refuses
//! or inflates, within the message limit and in time.
#![no_main]

use balaur_websocket::fuzzing::{handshake, inflate};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    handshake(data);
    let Some((extensions, mut rest)) = balaur_fuzz::text_then_bytes(data) else {
        return;
    };
    let mut payloads = Vec::new();
    while let [lo, hi, tail @ ..] = rest {
        let len = usize::from(u16::from_le_bytes([*lo, *hi])).min(tail.len());
        let (payload, next) = tail.split_at(len);
        payloads.push(payload);
        rest = next;
    }
    inflate(extensions, &payloads);
});

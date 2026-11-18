//! Any bytes as what a peer sends, one message a line, to a host in a
//! two-player match: each is read or refused and the ticks after it run.
#![no_main]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use balaur_core::{App, AppConfig};
use balaur_net::netsession::{NetSession, build_session_source};
use balaur_net::{Delivery, LinkState, Received, Transport};
use balaur_script::Value;
use libfuzzer_sys::fuzz_target;

const HOST: u32 = 1;
const GUEST: u32 = 2;
/// Ticks a snapshot is kept for, so a late input can still roll back.
const DEPTH: usize = 8;

/// The guest's end of the link: what the fuzzer says arrives, what the host
/// sends goes nowhere.
struct Peer(Rc<RefCell<VecDeque<Received>>>);

impl Transport for Peer {
    fn send_reliable(&mut self, _: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }

    fn send_datagram(&mut self, _: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }

    fn receive(&mut self) -> Vec<Received> {
        self.0.borrow_mut().drain(..).collect()
    }

    fn max_datagram(&self) -> usize {
        1200
    }

    fn state(&self) -> LinkState {
        LinkState::Open
    }

    fn close(&mut self) {}
}

fuzz_target!(|data: &[u8]| {
    let mut app = App::new(AppConfig::bare(".")).expect("a bare app starts");
    build_session_source(&mut app);
    let inbound = Rc::new(RefCell::new(VecDeque::new()));
    let mut host = NetSession::new(HOST, &[HOST, GUEST], DEPTH);
    host.add_peer(&app.engine, Box::new(Peer(Rc::clone(&inbound))));
    for (tick, line) in (0i64..).zip(data.split(|&b| b == b'\n')) {
        // Lines with an odd first byte come reliably, the rest as datagrams.
        let delivery = match line.first() {
            Some(b) if b % 2 == 1 => Delivery::Reliable,
            _ => Delivery::Datagram,
        };
        inbound.borrow_mut().push_back(Received {
            delivery,
            bytes: line.to_vec(),
        });
        host.set_input(Value::Int(tick));
        host.advance(&mut app);
        let _ = host.take_controls();
    }
});

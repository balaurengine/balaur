//! What writing and reading one property allocates: nothing, for the kinds a
//! script passes in its call's slots. A value built on the way is what a count
//! of allocations sees and a test of the result does not.
//!
//! Its own test binary, and one test in it: the counter below is the process's,
//! so a second test allocating beside it would be counted too.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use balaur_core::app::{App, AppConfig};
use balaur_core::components::derived::{Component, Enum, Given};
use balaur_core::components::{self, place_of};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract, passed straight through.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract, passed straight through.
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[derive(Clone, Copy, Debug, Enum)]
enum Mode {
    Slow,
    Fast,
}

/// One property of each kind a script's slots hold.
#[derive(Debug, Component)]
#[component(name = "gauge")]
struct Gauge {
    level: f32,
    wide: f64,
    count: i64,
    on: bool,
    at: [f32; 3],
    #[property(color)]
    tint: [f32; 4],
    #[property(default = Mode::Slow)]
    mode: Mode,
    label: smol_str::SmolStr,
}

#[test]
fn a_property_of_a_common_kind_moves_with_no_allocation() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    app.add_component::<Gauge>();
    let eng = &app.engine;
    let node = eng.spawn_node("Gauge", eng.root());
    components::add(eng, node, "gauge", None).unwrap();
    let writes: Vec<_> = [
        ("level", Given::Float(0.5)),
        ("wide", Given::Float(0.25)),
        ("count", Given::Int(9)),
        ("on", Given::Bool(true)),
        ("at", Given::Numbers([1.0, 2.0, 3.0, 0.0], 3)),
        ("tint", Given::Numbers([0.5, 0.25, 0.75, 1.0], 4)),
        ("mode", Given::Text("fast")),
        ("label", Given::Text("short")),
    ]
    .into_iter()
    .map(|(name, given)| (place_of(eng, "gauge", name).unwrap(), given))
    .collect();
    // The first write of a node's component makes its stamp.
    for (place, given) in &writes {
        components::set_property_placed(eng, node, place, *given).unwrap();
    }
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut read = 0;
    for _ in 0..100 {
        for (place, given) in &writes {
            components::set_property_placed(eng, node, place, *given).unwrap();
            components::given_placed(eng, node, place, &mut |_| read += 1);
        }
    }
    let spent = ALLOCATIONS.load(Ordering::Relaxed) - before;
    assert_eq!(read, 100 * writes.len(), "every property reads as it is");
    assert_eq!(
        spent, 0,
        "{spent} allocations over 800 writes and 800 reads"
    );
}

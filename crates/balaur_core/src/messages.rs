//! Typed messages between Rust systems (PLAN-ecs §2, Bevy's `Events`): one
//! system sends a value, every reader reads it once with a cursor of its
//! own. A message is kept for the frame it was sent in and the next, so a
//! reader that runs every frame hears each one whatever the order of
//! systems. Scripts keep their string events (`events`), which carry a
//! payload a script can read and reach a script a frame later.
//!
//! A message is not in a snapshot. A restore drops every one kept, the way a
//! restore marks every component changed: what was sent in a future the
//! rollback threw away is never heard, and the tick re-run sends again.

use std::collections::VecDeque;
use std::marker::PhantomData;

use crate::app::App;
use crate::engine::Engine;

/// Every message of one type still kept, numbered in the order sent.
pub struct Messages<T> {
    kept: VecDeque<T>,
    /// The number of the first message in `kept`.
    first: u64,
    /// The number the next one sent takes.
    next: u64,
    /// The numbers the last two frames started at: what `kept` keeps from.
    frames: [u64; 2],
}

impl<T> Default for Messages<T> {
    fn default() -> Self {
        Self {
            kept: VecDeque::new(),
            first: 0,
            next: 0,
            frames: [0; 2],
        }
    }
}

impl<T> Messages<T> {
    fn at(&mut self, edge: Edge) {
        match edge {
            // What was sent before the frame before this one goes.
            Edge::Frame => {
                self.frames = [self.frames[1], self.next];
                while self.first < self.frames[0] && self.kept.pop_front().is_some() {
                    self.first += 1;
                }
            }
            // Every one goes; the numbers go on, so no cursor is ahead.
            Edge::Restore => {
                self.kept.clear();
                self.first = self.next;
                self.frames = [self.next; 2];
            }
        }
    }
}

/// Where a type's kept messages are trimmed.
#[derive(Clone, Copy)]
enum Edge {
    Frame,
    Restore,
}

/// The message types an app declared, each one's trim.
#[derive(Default)]
pub(crate) struct MessageKinds(Vec<fn(&Engine, Edge)>);

/// Where one reader has read up to in one type's messages.
pub struct MessageCursor<T> {
    next: u64,
    marker: PhantomData<fn(&T)>,
}

impl<T> Default for MessageCursor<T> {
    fn default() -> Self {
        Self {
            next: 0,
            marker: PhantomData,
        }
    }
}

impl<T: 'static> MessageCursor<T> {
    /// Hand `each` every message sent since this cursor last read, oldest
    /// first, and move past them. One sent too long ago to be kept is
    /// skipped, and how many were is answered.
    ///
    /// # Panics
    /// If `T` was never declared with [`App::add_messages`].
    pub fn read(&mut self, eng: &Engine, mut each: impl FnMut(&T)) -> u64 {
        let messages = declared::<T>(eng);
        let messages = messages.borrow();
        let missed = messages.first.saturating_sub(self.next);
        let from = self.next.max(messages.first);
        for message in messages.kept.iter().skip((from - messages.first) as usize) {
            each(message);
        }
        self.next = messages.next;
        missed
    }

    /// [`Self::read`], into a list.
    pub fn read_cloned(&mut self, eng: &Engine) -> Vec<T>
    where
        T: Clone,
    {
        let mut out = Vec::new();
        self.read(eng, |message| out.push(message.clone()));
        out
    }
}

fn declared<T: 'static>(eng: &Engine) -> std::rc::Rc<std::cell::RefCell<Messages<T>>> {
    eng.try_resource::<Messages<T>>().unwrap_or_else(|| {
        panic!(
            "messages of {} were never declared with App::add_messages",
            std::any::type_name::<T>()
        )
    })
}

impl Engine {
    /// Send `message` to every reader of `T`.
    ///
    /// # Panics
    /// If `T` was never declared with [`App::add_messages`].
    pub fn send<T: 'static>(&self, message: T) {
        let messages = declared::<T>(self);
        let mut messages = messages.borrow_mut();
        messages.kept.push_back(message);
        messages.next += 1;
    }
}

impl App {
    /// Declare messages of type `T`: sent with [`Engine::send`], read with a
    /// [`MessageCursor`]. Declaring one twice is declaring it once.
    pub fn add_messages<T: 'static>(&mut self) -> &mut Self {
        if self.engine.try_resource::<Messages<T>>().is_some() {
            return self;
        }
        self.engine.insert_resource(Messages::<T>::default());
        if self.engine.try_resource::<MessageKinds>().is_none() {
            self.engine.insert_resource(MessageKinds::default());
        }
        self.engine
            .resource::<MessageKinds>()
            .borrow_mut()
            .0
            .push(|eng, edge| {
                if let Some(messages) = eng.try_resource::<Messages<T>>() {
                    messages.borrow_mut().at(edge);
                }
            });
        self
    }
}

/// A frame starts: every type's messages from before the frame before go.
pub(crate) fn frame_started(eng: &Engine) {
    trim(eng, Edge::Frame);
}

/// A snapshot was restored: every kept message goes.
pub(crate) fn restored(eng: &Engine) {
    trim(eng, Edge::Restore);
}

fn trim(eng: &Engine, edge: Edge) {
    let Some(kinds) = eng.try_resource::<MessageKinds>() else {
        return;
    };
    let trims = kinds.borrow().0.clone();
    for trim in trims {
        trim(eng, edge);
    }
}

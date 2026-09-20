//! `release.*`: which build this is, what the channels hold, and replacing
//! this install with one of them. The same code `balaur update` runs, so the
//! screen and the command cannot disagree.
//!
//! Reading the feed and installing are network work, so both are jobs: they
//! run on a thread and report through [`crate::jobs`], and the frame never
//! waits on GitHub.

use smol_str::SmolStr;
use std::cmp::Ordering;

use anyhow::Result;
use balaur::{Engine, Stage};
use balaur_script::{Bindings, BindingsExt, Value};
use serde::{Deserialize, Serialize};

use crate::jobs::{Reported, Reporting, install_listen, pump};
use crate::update::{Release, Step};

/// A download reports again once this many more bytes have arrived: often
/// enough for a bar, and not an event per network read.
const REPORT_EVERY: u64 = 1 << 20;

/// One step of a feed read or an install, crossing from the thread into a tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum ReleaseEvent {
    /// The feed, newest first, each release ordered against this build.
    Listed {
        rows: Vec<Row>,
    },
    Downloading {
        tag: String,
        done: u64,
        total: u64,
    },
    Unpacking {
        tag: String,
    },
    Installed {
        tag: String,
        note: String,
    },
    /// `job` is `check` or `install`, so a screen knows which press failed.
    Failed {
        job: String,
        message: String,
    },
}

/// A release as a screen shows it: what the feed said, and how it stands
/// against the build that asked.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Row {
    release: Release,
    /// `newer`, `older` or `same` than this build, or empty where the two
    /// do not order, as a nightly and a version do not.
    order: String,
    /// What a macOS bundle downloads instead of installing, else empty.
    download: String,
}

impl Row {
    fn of(release: Release) -> Self {
        let own = crate::version::build_id().unwrap_or_default();
        let order = if own == release.id {
            "same"
        } else {
            match crate::version::order(&release.id, own) {
                Some(Ordering::Greater) => "newer",
                Some(Ordering::Less) => "older",
                Some(Ordering::Equal) => "same",
                None => "",
            }
        };
        Self {
            download: crate::update::download_url(&release.tag),
            order: order.to_string(),
            release,
        }
    }

    fn value(&self) -> Value {
        let r = &self.release;
        Value::Map(vec![
            ("tag".into(), Value::Str(SmolStr::new(&r.tag))),
            ("id".into(), Value::Str(SmolStr::new(&r.id))),
            ("channel".into(), Value::Str(SmolStr::new(&r.channel))),
            ("when".into(), Value::Str(ago(&r.published).into())),
            ("current".into(), Value::Bool(self.order == "same")),
            ("order".into(), Value::Str(SmolStr::new(&self.order))),
            ("download".into(), Value::Str(SmolStr::new(&self.download))),
        ])
    }
}

/// A byte count as a script reads one: an integer, so a comparison with a
/// number written in a script does not meet a float.
fn bytes(n: u64) -> Value {
    Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

impl Reported for ReleaseEvent {
    fn value(&self) -> Value {
        let (kind, mut pairs): (&str, Vec<(SmolStr, Value)>) = match self {
            Self::Listed { rows } => (
                "listed",
                vec![(
                    "rows".into(),
                    Value::List(rows.iter().map(Row::value).collect()),
                )],
            ),
            Self::Downloading { tag, done, total } => (
                "downloading",
                vec![
                    ("tag".into(), Value::Str(SmolStr::new(tag))),
                    ("done".into(), bytes(*done)),
                    ("total".into(), bytes(*total)),
                ],
            ),
            Self::Unpacking { tag } => (
                "unpacking",
                vec![("tag".into(), Value::Str(SmolStr::new(tag)))],
            ),
            Self::Installed { tag, note } => (
                "installed",
                vec![
                    ("tag".into(), Value::Str(SmolStr::new(tag))),
                    ("note".into(), Value::Str(SmolStr::new(note))),
                ],
            ),
            Self::Failed { job, message } => (
                "failed",
                vec![
                    ("job".into(), Value::Str(SmolStr::new(job))),
                    ("message".into(), Value::Str(SmolStr::new(message))),
                ],
            ),
        };
        pairs.insert(0, ("kind".into(), Value::Str(kind.into())));
        Value::Map(pairs)
    }
}

/// Who hears a release job, and the channel its threads report on.
pub(crate) struct ReleaseState(Reporting<ReleaseEvent>);

impl AsMut<Reporting<ReleaseEvent>> for ReleaseState {
    fn as_mut(&mut self) -> &mut Reporting<ReleaseEvent> {
        &mut self.0
    }
}

/// The `release` module and the pump that delivers its jobs' reports.
pub(super) fn declare(reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
    reg.insert_resource(ReleaseState(Reporting::new(std::path::PathBuf::new())));
    reg.add_system(Stage::First, pump::<ReleaseState, ReleaseEvent>);
    let mut m = reg.script_module("release")?;
    install_release_api(&mut *m);
    Ok(())
}

fn install_release_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Which build of the engine this is, what the channels are publishing, and replacing this install with another. The editor's Engine tab and About sheet; `balaur update` is the same code.",
    );
    m.describe(&[
        (
            "installed",
            &[],
            "",
            "This build: `{ version, id, channel, tag, source, held }`, where `id` is the build id a release was tagged with, `tag` is the release its assets live under, `source` is true for a build from a checkout, and `held` says why this install cannot replace itself, empty when it can.",
        ),
        (
            "channels",
            &[],
            "",
            "Every release line a build may follow, in the order a version moves through them.",
        ),
        (
            "check",
            &[],
            "",
            "Read the release feed on a thread. `listen` hears `{ kind: \"listed\", rows }`, newest first, each `{ tag, id, channel, when, current, order, download }`: `order` is `newer`, `older` or `same` than this build, empty where they do not order, and `download` is the .dmg a macOS bundle fetches instead. `{ kind: \"failed\", job: \"check\", message }` when the feed could not be read. False while a recording plays.",
        ),
        (
            "install",
            &[],
            "(channel: string, tag: string, allow_downgrade: bool)",
            "Replace this install with what that channel or tag holds, on a thread. `listen` hears `downloading` with `done` and `total` bytes, `unpacking`, then `installed` with a `note`, or `failed` with `job: \"install\"`. Refuses where `installed().held` says why. False while a recording plays.",
        ),
        (
            "listen",
            &[],
            "(node: Node, opts?: table)",
            "Call a method on `node` with every report a check or an install makes: `on_release` unless `opts.on_event` names another.",
        ),
    ]);
    m.function("installed", |_: &Engine, ()| {
        let id = crate::version::build_id();
        Ok(Value::Map(vec![
            (
                "version".into(),
                Value::Str(env!("CARGO_PKG_VERSION").to_string().into()),
            ),
            (
                "id".into(),
                Value::Str(SmolStr::new(id.unwrap_or_default())),
            ),
            (
                "channel".into(),
                Value::Str(
                    crate::version::channel()
                        .unwrap_or_default()
                        .to_string()
                        .into(),
                ),
            ),
            (
                "tag".into(),
                Value::Str(
                    crate::version::release_tag()
                        .unwrap_or_default()
                        .to_string()
                        .into(),
                ),
            ),
            ("source".into(), Value::Bool(id.is_none())),
            (
                "held".into(),
                Value::Str(crate::update::held().unwrap_or_default().into()),
            ),
        ]))
    });
    m.function("channels", |_: &Engine, ()| {
        Ok(Value::List(
            crate::version::CHANNELS
                .iter()
                .map(|name| Value::Str((*name).to_string().into()))
                .collect(),
        ))
    });
    m.function("check", |eng: &Engine, ()| Ok(check(eng)));
    m.function(
        "install",
        |eng: &Engine, (channel, tag, allow_downgrade): (String, String, bool)| {
            Ok(install(eng, tag, channel, allow_downgrade))
        },
    );
    install_listen::<ReleaseState, ReleaseEvent>(m, "on_release");
}

/// Read the feed on a thread, unless a recording is playing.
fn check(eng: &Engine) -> bool {
    let state = eng.resource::<ReleaseState>();
    let state = state.borrow();
    state.0.io.start(eng, |report| {
        let report = report.clone();
        off_frame(move || {
            let event = match crate::update::releases() {
                Ok(found) => ReleaseEvent::Listed {
                    rows: found.into_iter().map(Row::of).collect(),
                },
                Err(e) => ReleaseEvent::Failed {
                    job: "check".into(),
                    message: format!("{e:#}"),
                },
            };
            let _ = report.send(event);
        });
    })
}

/// Install on a thread, unless a recording is playing. An empty string is
/// "not asked for": a script passes both and names one.
fn install(eng: &Engine, tag: String, channel: String, allow_downgrade: bool) -> bool {
    let state = eng.resource::<ReleaseState>();
    let state = state.borrow();
    state.0.io.start(eng, |report| {
        let report = report.clone();
        off_frame(move || {
            let named = if tag.is_empty() { &channel } else { &tag }.clone();
            let mut reported = 0u64;
            let mut progress = |step: Step| {
                let event = match step {
                    Step::Downloading(done, total) => {
                        if done < reported + REPORT_EVERY && done != total {
                            return;
                        }
                        reported = done;
                        ReleaseEvent::Downloading {
                            tag: named.clone(),
                            done,
                            total,
                        }
                    }
                    Step::Unpacking => ReleaseEvent::Unpacking { tag: named.clone() },
                };
                let _ = report.send(event);
            };
            let result = crate::update::replace(
                (!tag.is_empty()).then_some(tag.as_str()),
                (!channel.is_empty()).then_some(channel.as_str()),
                allow_downgrade,
                &mut progress,
            );
            let event = match result {
                Ok(note) => ReleaseEvent::Installed {
                    tag: named.clone(),
                    note,
                },
                Err(e) => ReleaseEvent::Failed {
                    job: "install".into(),
                    message: format!("{e:#}"),
                },
            };
            let _ = report.send(event);
        });
    })
}

/// A thread where there are threads. A browser tab has none, and every verb
/// there answers at once with why it cannot.
fn off_frame(work: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_family = "wasm"))]
    std::thread::spawn(work);
    #[cfg(target_family = "wasm")]
    work();
}

/// How long ago, in the words a person uses. The clock is the machine's,
/// which is what "yesterday" is measured against.
#[allow(
    clippy::disallowed_methods,
    reason = "orders a list a person reads, not simulation"
)]
fn ago(published: &str) -> String {
    let Some(then) = unix_of(published) else {
        return String::new();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().cast_signed());
    super::said_ago(now - then)
}

fn unix_of(stamp: &str) -> Option<i64> {
    let (date, rest) = stamp.split_once('T')?;
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    let mut clock = rest.trim_end_matches('Z').split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = clock.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    // Days since the epoch by the civil-from-days algorithm, which needs no
    // table and no leap-year special case past the one in it.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

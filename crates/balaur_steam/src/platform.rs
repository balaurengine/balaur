//! Steam as the store behind `platform.*`: who is playing and the Web API
//! ticket a server checks with Steam, achievements and leaderboards over
//! `ISteamUserStats`, cloud saves over `ISteamRemoteStorage`, presence over
//! `ISteamFriends`.
//!
//! A call Steam answers later is a call result: its handle goes in the
//! [`Inbox`] the steam plugin's pump fills, and the call answers when the
//! result lands, as GameKit's completion handlers answer on a Mac.
//! Achievements wait for the player's stats, which Steam fetches as the game
//! starts.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{CStr, CString};
use std::rc::Rc;
use std::sync::mpsc::Sender;

use balaur_platform::{Call, PlatformBackend, PlatformEvent, Player, Scope, Score};

use crate::client::{Heard, Inbox, Steam, Ticket};
use crate::marshal::read;
use crate::sdk::types::{
    EResult, LeaderboardEntry_t, LeaderboardFindResult_t, LeaderboardScoreUploaded_t,
    LeaderboardScoresDownloaded_t, RemoteStorageFileReadAsyncComplete_t,
    RemoteStorageFileWriteAsyncComplete_t, SteamAPICall_t, SteamLeaderboard_t,
    SteamLeaderboardEntries_t, UserStatsReceived_t, k_ELeaderboardDataRequestFriends,
    k_ELeaderboardDataRequestGlobal, k_ELeaderboardDisplayTypeNumeric,
    k_ELeaderboardSortMethodDescending, k_ELeaderboardUploadScoreMethodKeepBest, k_EResultFail,
    k_EResultFileNotFound, k_EResultOK, zeroed,
};

/// The rich-presence key the friends list shows with no localization file.
const STATUS: &CStr = c"status";
/// What `submit_score` answers `done` as.
const SUBMIT_SCORE: &str = "submit_score";
/// What `cloud_write` answers `done` as.
const CLOUD_WRITE: &str = "cloud_write";

/// Whether the player's stats are in, which every achievement call needs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stats {
    /// Neither heard nor asked for.
    Unknown,
    /// Asked for with `RequestUserStats`.
    Asked,
    Ready,
}

/// A call result in flight, and what its answer is for.
enum Flight {
    /// `RequestUserStats`, asked while achievement calls wait.
    Stats,
    /// `FindOrCreateLeaderboard`, on the way to posting `score`.
    FindToPost {
        request: u64,
        board: String,
        score: i32,
    },
    /// `UploadLeaderboardScore`.
    Post { request: u64, board: String },
    /// `FindLeaderboard`, on the way to reading it.
    FindToRead { request: u64, rows: Rows },
    /// `DownloadLeaderboardEntries`.
    Download { request: u64, rows: Rows },
    /// `FileReadAsync`.
    CloudRead { request: u64, key: String },
    /// `FileWriteAsync`, holding the bytes until Steam answers.
    CloudWrite {
        request: u64,
        key: String,
        _bytes: Vec<u8>,
    },
}

/// The rows a `scores` call asks for.
struct Rows {
    board: String,
    scope: Scope,
    /// The rank to read from, counting from 1.
    start: u32,
    count: u32,
}

pub(crate) struct SteamBackend {
    steam: Rc<Steam>,
    /// Filled by the steam plugin's own pump, which takes Steam's callbacks.
    inbox: Rc<RefCell<Inbox>>,
    /// The `identity` calls waiting for a ticket, oldest first: Steam answers
    /// them in the order they were asked.
    identities: VecDeque<u64>,
    stats: Stats,
    /// Achievement calls made before the stats came, in the order made.
    held: Vec<(u64, Call)>,
    /// Unlocks whose `StoreStats` has not answered, and the call each was.
    storing: Vec<(u64, &'static str)>,
    flights: BTreeMap<SteamAPICall_t, Flight>,
    /// Leaderboard handles by name, each found once.
    boards: BTreeMap<String, SteamLeaderboard_t>,
}

fn done(request: u64, call: &str) -> PlatformEvent {
    PlatformEvent::Done {
        request,
        call: call.to_string(),
    }
}

fn failed(request: u64, message: impl Into<String>) -> PlatformEvent {
    PlatformEvent::Failed {
        request,
        message: message.into(),
    }
}

fn lacks(request: u64, interface: &str) -> PlatformEvent {
    failed(request, format!("this steam_api has no {interface}"))
}

fn c_text(request: u64, text: &str) -> Result<CString, PlatformEvent> {
    CString::new(text).map_err(|_| failed(request, format!("{text:?} holds a NUL")))
}

fn send(report: &Sender<PlatformEvent>, event: PlatformEvent) {
    let _ = report.send(event);
}

/// A score as Steam keeps it, in 32 bits.
fn score_of(score: i64) -> i32 {
    i32::try_from(score.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or_default()
}

impl SteamBackend {
    pub(crate) fn new(steam: Rc<Steam>, inbox: Rc<RefCell<Inbox>>) -> Self {
        Self {
            steam,
            inbox,
            identities: VecDeque::new(),
            stats: Stats::Unknown,
            held: Vec::new(),
            storing: Vec::new(),
            flights: BTreeMap::new(),
            boards: BTreeMap::new(),
        }
    }

    /// This player's Steam id.
    fn me(&self) -> Option<u64> {
        let user = &self.steam.api.user;
        let (id, this) = (user.get_steam_id?, user.this()?);
        // SAFETY: bound after Steam started.
        Some(unsafe { id(this) })
    }

    fn player(&self) -> Option<Player> {
        let friends = &self.steam.api.friends;
        let (name, this) = (friends.get_persona_name?, friends.this()?);
        let id = self.me()?;
        // SAFETY: Steam keeps the name alive until the next call.
        let alias = match unsafe { crate::marshal::c_str_ptr(name(this)) } {
            balaur_script::Value::Str(alias) => alias,
            _ => String::new(),
        };
        Some(Player {
            id: id.to_string(),
            alias,
        })
    }

    /// The name a player shows, as far as this client knows it.
    fn alias_of(&self, id: u64) -> String {
        let friends = &self.steam.api.friends;
        let (Some(name), Some(this)) = (friends.get_friend_persona_name, friends.this()) else {
            return String::new();
        };
        // SAFETY: Steam keeps the name alive until the next call.
        match unsafe { crate::marshal::c_str_ptr(name(this, id)) } {
            balaur_script::Value::Str(alias) => alias,
            _ => String::new(),
        }
    }

    /// Wait for the call result `started` handed back: None when the library
    /// lacks the function, 0 when Steam refused the call.
    fn fly(
        &mut self,
        request: u64,
        started: Option<SteamAPICall_t>,
        expected: i32,
        flight: Flight,
        report: &Sender<PlatformEvent>,
    ) {
        match started {
            None => send(report, failed(request, "this steam_api lacks the function")),
            Some(0) => send(report, failed(request, "Steam refused the call")),
            Some(handle) => {
                self.inbox.borrow_mut().calls.insert(handle, expected);
                self.flights.insert(handle, flight);
            }
        }
    }
}

/// Achievements: an unlock stores the stats and answers once Steam says they
/// are stored; progress is the overlay's progress note.
impl SteamBackend {
    fn achievement(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>) {
        if self.stats == Stats::Ready {
            return self.award(request, call, report);
        }
        if self.stats == Stats::Unknown {
            if !self.ask_stats() {
                let why = "Steam would not fetch the player's stats";
                return send(report, failed(request, why));
            }
            self.stats = Stats::Asked;
        }
        self.held.push((request, call.clone()));
    }

    /// `RequestUserStats` for this player, whose answer frees the held calls.
    fn ask_stats(&mut self) -> bool {
        let stats = &self.steam.api.user_stats;
        let (Some(ask), Some(this), Some(me)) = (stats.request_user_stats, stats.this(), self.me())
        else {
            return false;
        };
        // SAFETY: bound after Steam started.
        let handle = unsafe { ask(this, me) };
        if handle == 0 {
            return false;
        }
        self.inbox
            .borrow_mut()
            .calls
            .insert(handle, UserStatsReceived_t::k_iCallback);
        self.flights.insert(handle, Flight::Stats);
        true
    }

    fn award(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>) {
        let event = match call {
            Call::Unlock { achievement } => self.unlock(request, achievement, call.name()),
            Call::Progress {
                achievement,
                percent,
            } if *percent >= 100.0 => self.unlock(request, achievement, call.name()),
            Call::Progress {
                achievement,
                percent,
            } => Some(self.progress(request, achievement, *percent, call.name())),
            _ => None,
        };
        if let Some(event) = event {
            send(report, event);
        }
    }

    /// `SetAchievement` then `StoreStats`; None while the store is on its way.
    fn unlock(&mut self, request: u64, name: &str, call: &'static str) -> Option<PlatformEvent> {
        let stats = &self.steam.api.user_stats;
        let (Some(set), Some(store), Some(this)) =
            (stats.set_achievement, stats.store_stats, stats.this())
        else {
            return Some(lacks(request, "ISteamUserStats"));
        };
        let id = match c_text(request, name) {
            Ok(id) => id,
            Err(event) => return Some(event),
        };
        // SAFETY: bound after Steam started; the name outlives the call.
        if !unsafe { set(this, id.as_ptr()) } {
            return Some(failed(request, format!("Steam has no achievement {name}")));
        }
        // SAFETY: as above.
        if !unsafe { store(this) } {
            return Some(failed(request, "Steam would not store the player's stats"));
        }
        self.storing.push((request, call));
        None
    }

    /// `IndicateAchievementProgress`, in the achievement's own progress range
    /// where it has one: a note Steam shows and keeps nowhere.
    fn progress(&self, request: u64, name: &str, percent: f64, call: &str) -> PlatformEvent {
        if percent <= 0.0 {
            return done(request, call);
        }
        let stats = &self.steam.api.user_stats;
        let (Some(indicate), Some(this)) = (stats.indicate_achievement_progress, stats.this())
        else {
            return lacks(request, "ISteamUserStats");
        };
        let id = match c_text(request, name) {
            Ok(id) => id,
            Err(event) => return event,
        };
        let (low, high) = self.limits(&id).unwrap_or((0, 100));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a share of a range Steam keeps in 32 bits"
        )]
        let current = (f64::from(low) + f64::from(high - low) * percent / 100.0) as u32;
        // SAFETY: bound after Steam started; the name outlives the call.
        if unsafe { indicate(this, id.as_ptr(), current, high.unsigned_abs()) } {
            return done(request, call);
        }
        if self.achieved(&id) {
            // Unlocked already, so there is nothing left to show.
            return done(request, call);
        }
        failed(request, format!("Steam has no achievement {name}"))
    }

    /// The range an achievement's progress stat runs over, where it has one.
    fn limits(&self, id: &CStr) -> Option<(i32, i32)> {
        let stats = &self.steam.api.user_stats;
        let (limits, this) = (stats.get_achievement_progress_limits_int32?, stats.this()?);
        let (mut low, mut high) = (0, 0);
        // SAFETY: bound after Steam started; out parameters of their types.
        let got = unsafe { limits(this, id.as_ptr(), &raw mut low, &raw mut high) };
        (got && high > low).then_some((low, high))
    }

    fn achieved(&self, id: &CStr) -> bool {
        let stats = &self.steam.api.user_stats;
        let (Some(get), Some(this)) = (stats.get_achievement, stats.this()) else {
            return false;
        };
        let mut achieved = false;
        // SAFETY: bound after Steam started; an out parameter of its type.
        let known = unsafe { get(this, id.as_ptr(), &raw mut achieved) };
        known && achieved
    }

    /// `UserStatsReceived_t` for this player: the held calls go out, or fail.
    fn stats_heard(&mut self, result: EResult, report: &Sender<PlatformEvent>) {
        let held = std::mem::take(&mut self.held);
        if result == k_EResultOK {
            self.stats = Stats::Ready;
            for (request, call) in held {
                self.award(request, &call, report);
            }
            return;
        }
        // The next achievement call asks again.
        self.stats = Stats::Unknown;
        for (request, _) in held {
            let message = format!("Steam did not hand over the player's stats: EResult {result}");
            send(report, failed(request, message));
        }
    }

    /// `UserStatsStored_t`: every unlock stored so far answers.
    fn stored(&mut self, result: EResult, report: &Sender<PlatformEvent>) {
        for (request, call) in std::mem::take(&mut self.storing) {
            let event = if result == k_EResultOK {
                done(request, call)
            } else {
                let message = format!("Steam did not store the player's stats: EResult {result}");
                failed(request, message)
            };
            send(report, event);
        }
    }
}

/// Leaderboards: each found once by name, then posted to or read.
impl SteamBackend {
    fn post(&mut self, request: u64, board: &str, score: i64, report: &Sender<PlatformEvent>) {
        let score = score_of(score);
        if let Some(&handle) = self.boards.get(board) {
            return self.upload(request, board, handle, score, report);
        }
        let name = match c_text(request, board) {
            Ok(name) => name,
            Err(event) => return send(report, event),
        };
        let stats = &self.steam.api.user_stats;
        let started = stats
            .find_or_create_leaderboard
            .zip(stats.this())
            .map(|(find, this)| {
                let (sort, display) = (
                    k_ELeaderboardSortMethodDescending,
                    k_ELeaderboardDisplayTypeNumeric,
                );
                // SAFETY: bound after Steam started; the name outlives the call.
                unsafe { find(this, name.as_ptr(), sort, display) }
            });
        let flight = Flight::FindToPost {
            request,
            board: board.to_string(),
            score,
        };
        let expected = LeaderboardFindResult_t::k_iCallback;
        self.fly(request, started, expected, flight, report);
    }

    /// `UploadLeaderboardScore`, keeping the player's best.
    fn upload(
        &mut self,
        request: u64,
        board: &str,
        handle: SteamLeaderboard_t,
        score: i32,
        report: &Sender<PlatformEvent>,
    ) {
        let stats = &self.steam.api.user_stats;
        let started = stats
            .upload_leaderboard_score
            .zip(stats.this())
            .map(|(upload, this)| {
                let best = k_ELeaderboardUploadScoreMethodKeepBest;
                // SAFETY: bound after Steam started; no details.
                unsafe { upload(this, handle, best, score, std::ptr::null(), 0) }
            });
        let flight = Flight::Post {
            request,
            board: board.to_string(),
        };
        let expected = LeaderboardScoreUploaded_t::k_iCallback;
        self.fly(request, started, expected, flight, report);
    }

    fn scores(&mut self, request: u64, rows: Rows, report: &Sender<PlatformEvent>) {
        if let Some(&handle) = self.boards.get(&rows.board) {
            return self.download(request, handle, rows, report);
        }
        let name = match c_text(request, &rows.board) {
            Ok(name) => name,
            Err(event) => return send(report, event),
        };
        let stats = &self.steam.api.user_stats;
        let started = stats
            .find_leaderboard
            .zip(stats.this())
            .map(|(find, this)| {
                // SAFETY: bound after Steam started; the name outlives the call.
                unsafe { find(this, name.as_ptr()) }
            });
        let flight = Flight::FindToRead { request, rows };
        let expected = LeaderboardFindResult_t::k_iCallback;
        self.fly(request, started, expected, flight, report);
    }

    /// `DownloadLeaderboardEntries`: ranks from `start` for the world, every
    /// friend for a friends read, which is then cut to the rows asked.
    fn download(
        &mut self,
        request: u64,
        handle: SteamLeaderboard_t,
        rows: Rows,
        report: &Sender<PlatformEvent>,
    ) {
        let first = i32::try_from(rows.start).unwrap_or(i32::MAX);
        let last = first.saturating_add(i32::try_from(rows.count.max(1)).unwrap_or(i32::MAX) - 1);
        let (kind, first, last) = match rows.scope {
            Scope::Global => (k_ELeaderboardDataRequestGlobal, first, last),
            Scope::Friends => (k_ELeaderboardDataRequestFriends, 0, 0),
        };
        let stats = &self.steam.api.user_stats;
        let started = stats
            .download_leaderboard_entries
            .zip(stats.this())
            .map(|(load, this)| {
                // SAFETY: bound after Steam started.
                unsafe { load(this, handle, kind, first, last) }
            });
        let flight = Flight::Download { request, rows };
        let expected = LeaderboardScoresDownloaded_t::k_iCallback;
        self.fly(request, started, expected, flight, report);
    }

    /// The rows a download holds, through `GetDownloadedLeaderboardEntry`.
    fn entries(&self, entries: SteamLeaderboardEntries_t, count: i32) -> Vec<Score> {
        let stats = &self.steam.api.user_stats;
        let (Some(get), Some(this)) = (stats.get_downloaded_leaderboard_entry, stats.this()) else {
            return Vec::new();
        };
        (0..count)
            .filter_map(|at| {
                let mut entry: LeaderboardEntry_t = zeroed();
                let details = std::ptr::null_mut();
                // SAFETY: bound after Steam started; no details asked for.
                let got = unsafe { get(this, entries, at, &raw mut entry, details, 0) };
                got.then(|| {
                    let id = { entry.m_steamIDUser }.0;
                    Score {
                        player: id.to_string(),
                        alias: self.alias_of(id),
                        rank: i64::from(entry.m_nGlobalRank),
                        score: i64::from(entry.m_nScore),
                    }
                })
            })
            .collect()
    }

    /// A `LeaderboardFindResult_t`: the board's handle, kept for next time.
    fn found(
        &mut self,
        data: Option<&[u8]>,
        board: &str,
        request: u64,
    ) -> Result<SteamLeaderboard_t, PlatformEvent> {
        match data.and_then(read::<LeaderboardFindResult_t>) {
            Some(found) if found.m_bLeaderboardFound != 0 => {
                let handle = found.m_hSteamLeaderboard;
                self.boards.insert(board.to_string(), handle);
                Ok(handle)
            }
            _ => Err(failed(request, format!("Steam has no leaderboard {board}"))),
        }
    }

    fn posted(data: Option<&[u8]>, request: u64, board: &str) -> PlatformEvent {
        match data.and_then(read::<LeaderboardScoreUploaded_t>) {
            Some(posted) if posted.m_bSuccess != 0 => done(request, SUBMIT_SCORE),
            _ => failed(request, format!("Steam did not take the score on {board}")),
        }
    }

    fn downloaded(&self, data: Option<&[u8]>, request: u64, rows: Rows) -> PlatformEvent {
        let Some(loaded) = data.and_then(read::<LeaderboardScoresDownloaded_t>) else {
            return failed(request, format!("Steam sent no entries for {}", rows.board));
        };
        let mut entries = self.entries(loaded.m_hSteamLeaderboardEntries, loaded.m_cEntryCount);
        if rows.scope == Scope::Friends {
            let skip = usize::try_from(rows.start.saturating_sub(1)).unwrap_or(usize::MAX);
            let take = usize::try_from(rows.count.max(1)).unwrap_or(usize::MAX);
            entries = entries.into_iter().skip(skip).take(take).collect();
        }
        PlatformEvent::Scores {
            request,
            board: rows.board,
            entries,
        }
    }
}

/// Cloud saves: a key is a file in the game's Steam Cloud, read and written
/// whole. Steam syncs it as the game starts and stops, and the last write
/// wins, as in iCloud's key-value store; a conflict between two machines is
/// Steam's own dialog at launch.
impl SteamBackend {
    fn cloud_read(&mut self, request: u64, key: &str, report: &Sender<PlatformEvent>) {
        let storage = &self.steam.api.remote_storage;
        let (Some(exists), Some(size), Some(read_async), Some(this)) = (
            storage.file_exists,
            storage.get_file_size,
            storage.file_read_async,
            storage.this(),
        ) else {
            return send(report, lacks(request, "ISteamRemoteStorage"));
        };
        let name = match c_text(request, key) {
            Ok(name) => name,
            Err(event) => return send(report, event),
        };
        let read_now = |value: Option<String>| PlatformEvent::Read {
            request,
            key: key.to_string(),
            value,
        };
        // SAFETY: bound after Steam started; the name outlives each call.
        if !unsafe { exists(this, name.as_ptr()) } {
            return send(report, read_now(None));
        }
        // SAFETY: as above.
        let bytes = match u32::try_from(unsafe { size(this, name.as_ptr()) }) {
            Ok(0) | Err(_) => return send(report, read_now(Some(String::new()))),
            Ok(bytes) => bytes,
        };
        // SAFETY: as above.
        let started = Some(unsafe { read_async(this, name.as_ptr(), 0, bytes) });
        let flight = Flight::CloudRead {
            request,
            key: key.to_string(),
        };
        let expected = RemoteStorageFileReadAsyncComplete_t::k_iCallback;
        self.fly(request, started, expected, flight, report);
    }

    fn cloud_write(
        &mut self,
        request: u64,
        key: &str,
        value: &str,
        report: &Sender<PlatformEvent>,
    ) {
        let name = match c_text(request, key) {
            Ok(name) => name,
            Err(event) => return send(report, event),
        };
        let bytes = value.as_bytes().to_vec();
        let Ok(len) = u32::try_from(bytes.len()) else {
            return send(report, failed(request, "a cloud value past 4 GB"));
        };
        let storage = &self.steam.api.remote_storage;
        let started = storage
            .file_write_async
            .zip(storage.this())
            .map(|(write, this)| {
                // SAFETY: bound after Steam started; the bytes stay in the flight
                // until Steam answers.
                unsafe { write(this, name.as_ptr(), bytes.as_ptr().cast(), len) }
            });
        let flight = Flight::CloudWrite {
            request,
            key: key.to_string(),
            _bytes: bytes,
        };
        let expected = RemoteStorageFileWriteAsyncComplete_t::k_iCallback;
        self.fly(request, started, expected, flight, report);
    }

    /// A finished read: `FileReadAsyncComplete` hands over its bytes.
    fn cloud_got(&self, data: Option<&[u8]>, request: u64, key: String) -> PlatformEvent {
        let Some(got) = data.and_then(read::<RemoteStorageFileReadAsyncComplete_t>) else {
            return failed(request, format!("Steam did not read {key}"));
        };
        let result = got.m_eResult;
        if result == k_EResultFileNotFound {
            let value = None;
            return PlatformEvent::Read {
                request,
                key,
                value,
            };
        }
        let storage = &self.steam.api.remote_storage;
        let (Some(take), Some(this)) = (storage.file_read_async_complete, storage.this()) else {
            return lacks(request, "ISteamRemoteStorage");
        };
        let size = got.m_cubRead;
        let mut bytes = vec![0_u8; usize::try_from(size).unwrap_or(0)];
        let (handle, into) = (got.m_hFileReadAsync, bytes.as_mut_ptr().cast());
        // SAFETY: bound after Steam started; a buffer of the size Steam read.
        let taken = result == k_EResultOK && unsafe { take(this, handle, into, size) };
        if !taken {
            return failed(
                request,
                format!("Steam did not read {key}: EResult {result}"),
            );
        }
        match String::from_utf8(bytes) {
            Ok(text) => PlatformEvent::Read {
                request,
                key,
                value: Some(text),
            },
            Err(_) => failed(request, format!("{key} in Steam Cloud is not text")),
        }
    }

    fn cloud_written(data: Option<&[u8]>, request: u64, key: &str) -> PlatformEvent {
        let result = data
            .and_then(read::<RemoteStorageFileWriteAsyncComplete_t>)
            .map_or(k_EResultFail, |written| written.m_eResult);
        if result == k_EResultOK {
            done(request, CLOUD_WRITE)
        } else {
            failed(
                request,
                format!("Steam did not write {key}: EResult {result}"),
            )
        }
    }

    /// `SetRichPresence` of `status`, which the friends list shows.
    fn set_presence(&self, request: u64, text: &str, call: &str) -> PlatformEvent {
        let friends = &self.steam.api.friends;
        let (Some(set), Some(this)) = (friends.set_rich_presence, friends.this()) else {
            return lacks(request, "ISteamFriends");
        };
        let value = match c_text(request, text) {
            Ok(value) => value,
            Err(event) => return event,
        };
        // SAFETY: bound after Steam started; both strings outlive the call.
        if unsafe { set(this, STATUS.as_ptr(), value.as_ptr()) } {
            done(request, call)
        } else {
            failed(request, "Steam refused the presence: at most 256 bytes")
        }
    }
}

/// What Steam's answers do once the steam plugin's pump has taken them.
impl SteamBackend {
    fn heard(&mut self, heard: Heard, report: &Sender<PlatformEvent>) {
        match heard {
            Heard::Ticket(ticket) => self.ticket(ticket, report),
            Heard::Stats { user, result } if Some(user) == self.me() => {
                self.stats_heard(result, report);
            }
            Heard::Stats { .. } => {}
            Heard::Stored { result } => self.stored(result, report),
            Heard::Answer { handle, data } => self.landed(handle, data.as_deref(), report),
        }
    }

    fn ticket(&mut self, ticket: Ticket, report: &Sender<PlatformEvent>) {
        let Some(request) = self.identities.pop_front() else {
            return;
        };
        let event = match ticket {
            Ok(bytes) => PlatformEvent::Identity {
                request,
                proof: serde_json::json!({ "ticket": super::hex(&bytes) }),
            },
            Err(message) => PlatformEvent::Failed { request, message },
        };
        send(report, event);
    }

    /// A call result the backend waited on has landed.
    fn landed(
        &mut self,
        handle: SteamAPICall_t,
        data: Option<&[u8]>,
        report: &Sender<PlatformEvent>,
    ) {
        let Some(flight) = self.flights.remove(&handle) else {
            return;
        };
        let event = match flight {
            Flight::Stats => {
                let result = data
                    .and_then(read::<UserStatsReceived_t>)
                    .map_or(k_EResultFail, |got| got.m_eResult);
                return self.stats_heard(result, report);
            }
            Flight::FindToPost {
                request,
                board,
                score,
            } => match self.found(data, &board, request) {
                Ok(found) => return self.upload(request, &board, found, score, report),
                Err(event) => event,
            },
            Flight::Post { request, board } => Self::posted(data, request, &board),
            Flight::FindToRead { request, rows } => match self.found(data, &rows.board, request) {
                Ok(found) => return self.download(request, found, rows, report),
                Err(event) => event,
            },
            Flight::Download { request, rows } => self.downloaded(data, request, rows),
            Flight::CloudRead { request, key } => self.cloud_got(data, request, key),
            Flight::CloudWrite { request, key, .. } => Self::cloud_written(data, request, &key),
        };
        send(report, event);
    }
}

impl PlatformBackend for SteamBackend {
    fn name(&self) -> &'static str {
        super::STEAM
    }

    fn start(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>) {
        match call {
            Call::SignIn => {
                let event = match self.player() {
                    Some(player) => PlatformEvent::SignedIn { request, player },
                    None => lacks(request, "ISteamUser or ISteamFriends"),
                };
                send(report, event);
            }
            Call::Identity if self.steam.ask_web_ticket() => self.identities.push_back(request),
            Call::Unlock { .. } | Call::Progress { .. } => self.achievement(request, call, report),
            Call::SubmitScore { board, score } => self.post(request, board, *score, report),
            // Steam keeps one table a leaderboard, whatever the period.
            Call::Scores {
                board,
                count,
                scope,
                start,
                ..
            } => {
                let rows = Rows {
                    board: board.clone(),
                    scope: *scope,
                    start: *start,
                    count: *count,
                };
                self.scores(request, rows, report);
            }
            Call::CloudRead { key } => self.cloud_read(request, key, report),
            Call::CloudWrite { key, value } => self.cloud_write(request, key, value, report),
            Call::SetPresence { text } => {
                send(report, self.set_presence(request, text, call.name()));
            }
            // Steam has no sign-in sheet: the overlay's browser hands no
            // redirect back, so the game opens the page itself. An identity
            // lands here when this steam_api has no ISteamUser.
            Call::WebSignIn { .. } | Call::Identity => send(
                report,
                PlatformEvent::Unsupported {
                    request,
                    call: call.name().to_string(),
                },
            ),
        }
    }

    fn pump(&mut self, report: &Sender<PlatformEvent>) {
        let arrived: Vec<Heard> = self.inbox.borrow_mut().arrived.drain(..).collect();
        for heard in arrived {
            self.heard(heard, report);
        }
        // A stats answer a suppressed tick dropped is asked for again.
        if self.stats == Stats::Unknown && !self.held.is_empty() && self.ask_stats() {
            self.stats = Stats::Asked;
        }
    }

    fn discard(&mut self) {
        self.inbox.borrow_mut().arrived.clear();
        self.identities.clear();
        if self.stats == Stats::Asked {
            self.stats = Stats::Unknown;
        }
    }
}

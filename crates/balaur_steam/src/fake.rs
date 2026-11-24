//! A stand-in for Valve's library, for tests: the flat functions the engine
//! calls, under their own symbol names, answering from this thread's
//! [`World`] and queueing callbacks and call results as a client would.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{CStr, c_char, c_void};

use crate::client::Steam;
use crate::library::Library;
use crate::sdk::types::{
    CSteamID, CallbackMsg_t, ELeaderboardDataRequest, ELeaderboardDisplayType,
    ELeaderboardSortMethod, ELeaderboardUploadScoreMethod, ESteamAPIInitResult,
    GetTicketForWebApiResponse_t, HAuthTicket, HSteamPipe, LeaderboardEntry_t,
    LeaderboardFindResult_t, LeaderboardScoreUploaded_t, LeaderboardScoresDownloaded_t, Plain,
    RemoteStorageFileReadAsyncComplete_t, RemoteStorageFileWriteAsyncComplete_t, SteamAPICall_t,
    SteamAPICallCompleted_t, SteamErrMsg, SteamLeaderboard_t, SteamLeaderboardEntries_t,
    UserStatsReceived_t, UserStatsStored_t, k_EResultFileNotFound, k_EResultOK,
    k_ESteamAPIInitResult_OK, zeroed,
};

/// The player the fake client runs as.
pub(crate) const ME: u64 = 76_561_197_960_287_930;
/// A friend on every leaderboard.
pub(crate) const FRIEND: u64 = 76_561_197_960_287_931;
/// The one achievement with a progress stat, running from 0 to 10.
pub(crate) const TEN_WINS: &str = "ten_wins";

/// What the fake client holds and what it was asked, per test thread.
#[derive(Default)]
pub(crate) struct World {
    /// Every call made, as `Function argument`.
    pub(crate) log: Vec<String>,
    /// Achievement names to whether each is unlocked.
    pub(crate) achievements: BTreeMap<String, bool>,
    /// Leaderboards by name: each row a player and their score, best first.
    pub(crate) boards: BTreeMap<String, Vec<(u64, i32)>>,
    pub(crate) files: BTreeMap<String, Vec<u8>>,
    pub(crate) presence: BTreeMap<String, String>,
    callbacks: VecDeque<(i32, Vec<u8>)>,
    results: BTreeMap<SteamAPICall_t, Vec<u8>>,
    handles: u64,
    /// The callback handed out last, until `FreeLastCallback`.
    given: Vec<u8>,
    /// Each download's rows, by its entries handle.
    downloads: BTreeMap<SteamLeaderboardEntries_t, Vec<(u64, i32, i32)>>,
    /// Each read's file, by its handle.
    reads: BTreeMap<SteamAPICall_t, String>,
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World::default());
}

/// Read or change this thread's fake client.
pub(crate) fn with<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with_borrow_mut(f)
}

fn bytes<T: Plain>(value: &T) -> Vec<u8> {
    // SAFETY: plain data, read as the bytes it is.
    unsafe { std::slice::from_raw_parts(std::ptr::from_ref(value).cast::<u8>(), size_of::<T>()) }
        .to_vec()
}

impl World {
    /// Queue a callback, as Steam posts one.
    pub(crate) fn post<T: Plain>(&mut self, id: i32, value: &T) {
        self.callbacks.push_back((id, bytes(value)));
    }

    /// Finish a call result on the spot: its struct kept, and the
    /// `SteamAPICallCompleted_t` that says so queued.
    fn answer<T: Plain>(&mut self, id: i32, value: &T) -> SteamAPICall_t {
        self.handles += 1;
        let handle = self.handles;
        let data = bytes(value);
        let mut done: SteamAPICallCompleted_t = zeroed();
        done.m_hAsyncCall = handle;
        done.m_iCallback = id;
        done.m_cubParam = u32::try_from(data.len()).unwrap_or(0);
        self.results.insert(handle, data);
        self.post(SteamAPICallCompleted_t::k_iCallback, &done);
        handle
    }

    fn said(&mut self, line: String) {
        self.log.push(line);
    }
}

/// Steam's own stats arrival for this player, as the client posts it at start.
pub(crate) fn stats_arrive() {
    let mut got: UserStatsReceived_t = zeroed();
    got.m_eResult = k_EResultOK;
    got.m_steamIDUser = CSteamID(ME);
    with(|world| world.post(UserStatsReceived_t::k_iCallback, &got));
}

fn text(ptr: *const c_char) -> String {
    // SAFETY: the engine passes NUL-terminated strings.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// Any interface's object: a fixed address the functions never read.
static OBJECT: u8 = 0;

extern "C" fn object() -> *mut c_void {
    std::ptr::from_ref(&OBJECT).cast_mut().cast()
}

extern "C" fn init_flat(_: *mut SteamErrMsg) -> ESteamAPIInitResult {
    k_ESteamAPIInitResult_OK
}

extern "C" fn nothing() {}

extern "C" fn pipe() -> HSteamPipe {
    1
}

extern "C" fn run_frame(_: HSteamPipe) {}

extern "C" fn next_callback(_: HSteamPipe, message: *mut CallbackMsg_t) -> bool {
    with(|world| {
        let Some((id, data)) = world.callbacks.pop_front() else {
            return false;
        };
        world.given = data;
        let mut out: CallbackMsg_t = zeroed();
        out.m_iCallback = id;
        out.m_pubParam = world.given.as_mut_ptr();
        out.m_cubParam = i32::try_from(world.given.len()).unwrap_or(0);
        // SAFETY: the engine's message to fill.
        unsafe { message.write_unaligned(out) };
        true
    })
}

extern "C" fn free_last(_: HSteamPipe) {
    with(|world| world.given.clear());
}

extern "C" fn call_result(
    _: HSteamPipe,
    handle: SteamAPICall_t,
    into: *mut c_void,
    size: i32,
    _: i32,
    failed: *mut bool,
) -> bool {
    let Some(data) = with(|world| world.results.remove(&handle)) else {
        return false;
    };
    let len = data.len().min(usize::try_from(size).unwrap_or(0));
    // SAFETY: the engine's buffer of `size` bytes, and its flag.
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), into.cast(), len);
        failed.write(false);
    }
    true
}

extern "C" fn steam_id(_: *mut c_void) -> u64 {
    ME
}

extern "C" fn persona_name(_: *mut c_void) -> *const c_char {
    c"Ada".as_ptr()
}

extern "C" fn friend_name(_: *mut c_void, id: u64) -> *const c_char {
    if id == ME { c"Ada" } else { c"Bea" }.as_ptr()
}

extern "C" fn web_ticket(_: *mut c_void, _: *const c_char) -> HAuthTicket {
    let mut answer: GetTicketForWebApiResponse_t = zeroed();
    answer.m_hAuthTicket = 7;
    answer.m_eResult = k_EResultOK;
    answer.m_cubTicket = 3;
    let mut ticket = answer.m_rgubTicket;
    ticket[..3].copy_from_slice(&[0x14, 0x00, 0xAB]);
    answer.m_rgubTicket = ticket;
    with(|world| world.post(GetTicketForWebApiResponse_t::k_iCallback, &answer));
    7
}

extern "C" fn set_achievement(_: *mut c_void, name: *const c_char) -> bool {
    let name = text(name);
    with(|world| {
        world.said(format!("SetAchievement {name}"));
        world
            .achievements
            .get_mut(&name)
            .map(|unlocked| *unlocked = true)
            .is_some()
    })
}

extern "C" fn get_achievement(_: *mut c_void, name: *const c_char, achieved: *mut bool) -> bool {
    let Some(unlocked) = with(|world| world.achievements.get(&text(name)).copied()) else {
        return false;
    };
    // SAFETY: the engine's out parameter.
    unsafe { achieved.write(unlocked) };
    true
}

extern "C" fn store_stats(_: *mut c_void) -> bool {
    let mut stored: UserStatsStored_t = zeroed();
    stored.m_eResult = k_EResultOK;
    with(|world| {
        world.said("StoreStats".into());
        world.post(UserStatsStored_t::k_iCallback, &stored);
    });
    true
}

extern "C" fn indicate(_: *mut c_void, name: *const c_char, current: u32, max: u32) -> bool {
    let name = text(name);
    with(|world| {
        world.said(format!(
            "IndicateAchievementProgress {name} {current}/{max}"
        ));
        world.achievements.get(&name) == Some(&false)
    })
}

extern "C" fn limits(_: *mut c_void, name: *const c_char, low: *mut i32, high: *mut i32) -> bool {
    if text(name) != TEN_WINS {
        return false;
    }
    // SAFETY: the engine's out parameters.
    unsafe {
        low.write(0);
        high.write(10);
    }
    true
}

extern "C" fn request_user_stats(_: *mut c_void, user: u64) -> SteamAPICall_t {
    let mut got: UserStatsReceived_t = zeroed();
    got.m_eResult = k_EResultOK;
    got.m_steamIDUser = CSteamID(user);
    with(|world| {
        world.said(format!("RequestUserStats {user}"));
        world.answer(UserStatsReceived_t::k_iCallback, &got)
    })
}

/// A board's handle: its place in the world's list, from 1.
fn board_handle(world: &World, name: &str) -> SteamLeaderboard_t {
    world
        .boards
        .keys()
        .position(|board| board == name)
        .map_or(0, |at| at as u64 + 1)
}

fn find(name: &str, create: bool) -> SteamAPICall_t {
    with(|world| {
        let verb = if create {
            "FindOrCreateLeaderboard"
        } else {
            "FindLeaderboard"
        };
        world.said(format!("{verb} {name}"));
        if create {
            world.boards.entry(name.to_string()).or_default();
        }
        let mut found: LeaderboardFindResult_t = zeroed();
        found.m_hSteamLeaderboard = board_handle(world, name);
        found.m_bLeaderboardFound = u8::from(found.m_hSteamLeaderboard != 0);
        world.answer(LeaderboardFindResult_t::k_iCallback, &found)
    })
}

extern "C" fn find_or_create(
    _: *mut c_void,
    name: *const c_char,
    _: ELeaderboardSortMethod,
    _: ELeaderboardDisplayType,
) -> SteamAPICall_t {
    find(&text(name), true)
}

extern "C" fn find_board(_: *mut c_void, name: *const c_char) -> SteamAPICall_t {
    find(&text(name), false)
}

fn board_name(world: &World, handle: SteamLeaderboard_t) -> Option<String> {
    let at = usize::try_from(handle).ok()?.checked_sub(1)?;
    world.boards.keys().nth(at).cloned()
}

extern "C" fn upload(
    _: *mut c_void,
    handle: SteamLeaderboard_t,
    _: ELeaderboardUploadScoreMethod,
    score: i32,
    _: *const i32,
    _: i32,
) -> SteamAPICall_t {
    with(|world| {
        let name = board_name(world, handle).unwrap_or_default();
        world.said(format!("UploadLeaderboardScore {name} {score}"));
        let rows = world.boards.entry(name).or_default();
        rows.retain(|(player, _)| *player != ME);
        rows.push((ME, score));
        rows.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
        let mut posted: LeaderboardScoreUploaded_t = zeroed();
        posted.m_bSuccess = 1;
        posted.m_hSteamLeaderboard = handle;
        posted.m_nScore = score;
        world.answer(LeaderboardScoreUploaded_t::k_iCallback, &posted)
    })
}

extern "C" fn download(
    _: *mut c_void,
    handle: SteamLeaderboard_t,
    kind: ELeaderboardDataRequest,
    first: i32,
    last: i32,
) -> SteamAPICall_t {
    with(|world| {
        let name = board_name(world, handle).unwrap_or_default();
        world.said(format!(
            "DownloadLeaderboardEntries {name} {kind} {first}..{last}"
        ));
        let ranked = world.boards.get(&name).cloned().unwrap_or_default();
        let rows: Vec<(u64, i32, i32)> = (1..)
            .zip(ranked)
            .filter(|(rank, _)| kind != 0 || (first..=last).contains(rank))
            .map(|(rank, (player, score))| (player, rank, score))
            .collect();
        let entries = 1000 + world.handles;
        let mut loaded: LeaderboardScoresDownloaded_t = zeroed();
        loaded.m_hSteamLeaderboard = handle;
        loaded.m_hSteamLeaderboardEntries = entries;
        loaded.m_cEntryCount = i32::try_from(rows.len()).unwrap_or(0);
        world.downloads.insert(entries, rows);
        world.answer(LeaderboardScoresDownloaded_t::k_iCallback, &loaded)
    })
}

extern "C" fn downloaded_entry(
    _: *mut c_void,
    entries: SteamLeaderboardEntries_t,
    at: i32,
    entry: *mut LeaderboardEntry_t,
    _: *mut i32,
    _: i32,
) -> bool {
    let row = with(|world| {
        let rows = world.downloads.get(&entries)?;
        rows.get(usize::try_from(at).ok()?).copied()
    });
    let Some((player, rank, score)) = row else {
        return false;
    };
    let mut out: LeaderboardEntry_t = zeroed();
    out.m_steamIDUser = CSteamID(player);
    out.m_nGlobalRank = rank;
    out.m_nScore = score;
    // SAFETY: the engine's out parameter.
    unsafe { entry.write_unaligned(out) };
    true
}

extern "C" fn file_exists(_: *mut c_void, name: *const c_char) -> bool {
    with(|world| world.files.contains_key(&text(name)))
}

extern "C" fn file_size(_: *mut c_void, name: *const c_char) -> i32 {
    with(|world| {
        world
            .files
            .get(&text(name))
            .map_or(0, |data| i32::try_from(data.len()).unwrap_or(0))
    })
}

extern "C" fn read_async(_: *mut c_void, name: *const c_char, _: u32, size: u32) -> SteamAPICall_t {
    let name = text(name);
    with(|world| {
        world.said(format!("FileReadAsync {name}"));
        let found = world.files.get(&name).map(Vec::len);
        let mut got: RemoteStorageFileReadAsyncComplete_t = zeroed();
        // The handle `answer` gives next, which a read is known by.
        got.m_hFileReadAsync = world.handles + 1;
        got.m_eResult = if found.is_some() {
            k_EResultOK
        } else {
            k_EResultFileNotFound
        };
        got.m_cubRead = found.map_or(0, |len| size.min(u32::try_from(len).unwrap_or(0)));
        world.reads.insert(got.m_hFileReadAsync, name);
        world.answer(RemoteStorageFileReadAsyncComplete_t::k_iCallback, &got)
    })
}

extern "C" fn read_async_complete(
    _: *mut c_void,
    handle: SteamAPICall_t,
    into: *mut c_void,
    size: u32,
) -> bool {
    let data = with(|world| {
        let name = world.reads.remove(&handle)?;
        world.files.get(&name).cloned()
    });
    let Some(data) = data else {
        return false;
    };
    let len = data.len().min(usize::try_from(size).unwrap_or(0));
    // SAFETY: the engine's buffer of `size` bytes.
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), into.cast(), len) };
    true
}

extern "C" fn write_async(
    _: *mut c_void,
    name: *const c_char,
    data: *const c_void,
    size: u32,
) -> SteamAPICall_t {
    let name = text(name);
    let len = usize::try_from(size).unwrap_or(0);
    // SAFETY: the engine's `size` bytes.
    let data = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) }.to_vec();
    with(|world| {
        world.said(format!("FileWriteAsync {name} {len}"));
        world.files.insert(name, data);
        let mut written: RemoteStorageFileWriteAsyncComplete_t = zeroed();
        written.m_eResult = k_EResultOK;
        world.answer(RemoteStorageFileWriteAsyncComplete_t::k_iCallback, &written)
    })
}

extern "C" fn rich_presence(_: *mut c_void, key: *const c_char, value: *const c_char) -> bool {
    let (key, value) = (text(key), text(value));
    with(|world| {
        world.said(format!("SetRichPresence {key} {value}"));
        world.presence.insert(key, value);
    });
    true
}

/// Each symbol under its name as the engine looks it up, NUL and all.
macro_rules! symbols {
    ($($name:ident => $function:expr,)*) => {
        [$((concat!(stringify!($name), "\0").as_bytes(), $function as *const () as usize),)*]
    };
}

/// Every symbol the fake answers.
fn symbols() -> BTreeMap<&'static [u8], usize> {
    let accessor = object;
    symbols! {
        SteamAPI_InitFlat => init_flat,
        SteamAPI_Shutdown => nothing,
        SteamAPI_GetHSteamPipe => pipe,
        SteamAPI_ManualDispatch_Init => nothing,
        SteamAPI_ManualDispatch_RunFrame => run_frame,
        SteamAPI_ManualDispatch_GetNextCallback => next_callback,
        SteamAPI_ManualDispatch_FreeLastCallback => free_last,
        SteamAPI_ManualDispatch_GetAPICallResult => call_result,
        SteamAPI_SteamUser_v023 => accessor,
        SteamAPI_SteamFriends_v018 => accessor,
        SteamAPI_SteamUserStats_v013 => accessor,
        SteamAPI_SteamRemoteStorage_v016 => accessor,
        SteamAPI_ISteamUser_GetSteamID => steam_id,
        SteamAPI_ISteamUser_GetAuthTicketForWebApi => web_ticket,
        SteamAPI_ISteamFriends_GetPersonaName => persona_name,
        SteamAPI_ISteamFriends_GetFriendPersonaName => friend_name,
        SteamAPI_ISteamFriends_SetRichPresence => rich_presence,
        SteamAPI_ISteamUserStats_SetAchievement => set_achievement,
        SteamAPI_ISteamUserStats_GetAchievement => get_achievement,
        SteamAPI_ISteamUserStats_StoreStats => store_stats,
        SteamAPI_ISteamUserStats_IndicateAchievementProgress => indicate,
        SteamAPI_ISteamUserStats_GetAchievementProgressLimitsInt32 => limits,
        SteamAPI_ISteamUserStats_RequestUserStats => request_user_stats,
        SteamAPI_ISteamUserStats_FindOrCreateLeaderboard => find_or_create,
        SteamAPI_ISteamUserStats_FindLeaderboard => find_board,
        SteamAPI_ISteamUserStats_UploadLeaderboardScore => upload,
        SteamAPI_ISteamUserStats_DownloadLeaderboardEntries => download,
        SteamAPI_ISteamUserStats_GetDownloadedLeaderboardEntry => downloaded_entry,
        SteamAPI_ISteamRemoteStorage_FileExists => file_exists,
        SteamAPI_ISteamRemoteStorage_GetFileSize => file_size,
        SteamAPI_ISteamRemoteStorage_FileReadAsync => read_async,
        SteamAPI_ISteamRemoteStorage_FileReadAsyncComplete => read_async_complete,
        SteamAPI_ISteamRemoteStorage_FileWriteAsync => write_async,
    }
    .into_iter()
    .collect()
}

/// A client started on the fake library, with this thread's world reset to
/// `world`.
pub(crate) fn steam(world: World) -> Steam {
    with(|held| *held = world);
    // SAFETY: each symbol is a function of the type its name has.
    unsafe { Steam::start_with(Library::fake(symbols())) }
        .expect("the fake has every start-up function")
}

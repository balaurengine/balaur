//! Game Center: who is playing, achievements, leaderboards, and the four
//! items a server checks a player with.
//!
//! Calls are made from the frame loop, which is the main thread — GameKit
//! presents its own sheets and expects that. Completions come back on
//! whatever queue GameKit chose, so each block owns its own `Sender` clone
//! and the frame loop never reads one from two threads.

use std::sync::mpsc::Sender;

use balaur_platform::{Call, PlatformEvent, Player, Score};
use block2::RcBlock;
use objc2::runtime::AnyObject;
use objc2::{msg_send, AllocAnyThread};
use objc2_foundation::{NSArray, NSError, NSRange, NSString};
use objc2_game_kit::{
    GKAchievement, GKLeaderboard, GKLeaderboardEntry, GKLeaderboardPlayerScope,
    GKLeaderboardTimeScope, GKLocalPlayer,
};

use crate::{AppleCall, AppleEvent};

/// Whether Game Center has a signed-in player right now.
pub(crate) fn authenticated() -> bool {
    unsafe { GKLocalPlayer::localPlayer().isAuthenticated() }
}

pub(crate) fn platform_call(request: u64, call: &Call, report: &Sender<PlatformEvent>) {
    match call {
        Call::SignIn => sign_in(request, report),
        Call::Unlock { achievement } => report_achievement(request, achievement, 100.0, report),
        Call::Progress {
            achievement,
            percent,
        } => report_achievement(request, achievement, *percent, report),
        Call::SubmitScore { board, score } => submit_score(request, board, *score, report),
        Call::Scores { board, count } => scores(request, board, *count, report),
        Call::CloudRead { key } => crate::icloud::read(request, key, report),
        Call::CloudWrite { key, value } => crate::icloud::write(request, key, value, report),
        // Game Center has no presence, and answering `done` to a call that
        // did nothing is worse than saying so.
        Call::SetPresence { .. } => {
            let _ = report.send(PlatformEvent::Unsupported {
                request,
                call: call.name().to_string(),
            });
        }
    }
}

pub(crate) fn apple_call(request: u64, call: &AppleCall, report: &Sender<AppleEvent>) {
    match call {
        AppleCall::Identity => identity(request, report),
        AppleCall::SignIn => crate::signin::sign_in(request, report),
    }
}

fn sign_in(request: u64, report: &Sender<PlatformEvent>) {
    let player = unsafe { GKLocalPlayer::localPlayer() };
    if unsafe { player.isAuthenticated() } {
        let _ = report.send(signed_in(request, &player));
        return;
    }
    let report = report.clone();
    // GameKit hands the handler a view controller when it wants its sign-in
    // sheet. Nothing presents one yet (docs/PLAN-apple.md step 5), so that
    // case is reported rather than swallowed.
    let handler = RcBlock::new(move |sheet: *mut AnyObject, error: *mut NSError| {
        let player = unsafe { GKLocalPlayer::localPlayer() };
        let event = if unsafe { player.isAuthenticated() } {
            signed_in(request, &player)
        } else if let Some(error) = unsafe { error.as_ref() } {
            PlatformEvent::Failed {
                request,
                message: describe(error),
            }
        } else if sheet.is_null() {
            PlatformEvent::Failed {
                request,
                message: "Game Center has no signed-in player".into(),
            }
        } else {
            PlatformEvent::Failed {
                request,
                message: "Game Center wants its sign-in sheet, which nothing presents yet".into(),
            }
        };
        let _ = report.send(event);
    });
    // Raw, because the typed binding takes the platform's own view-controller
    // type and so needs AppKit on macOS and UIKit on iOS; the block's ABI is
    // the same either way and the controller is not used.
    unsafe {
        let _: () = msg_send![&player, setAuthenticateHandler: &*handler];
    }
}

fn signed_in(request: u64, player: &GKLocalPlayer) -> PlatformEvent {
    PlatformEvent::SignedIn {
        request,
        player: Player {
            id: unsafe { player.gamePlayerID() }.to_string(),
            alias: unsafe { player.alias() }.to_string(),
        },
    }
}

fn report_achievement(request: u64, id: &str, percent: f64, report: &Sender<PlatformEvent>) {
    let call = if percent >= 100.0 {
        "unlock"
    } else {
        "progress"
    };
    let achievement = unsafe {
        GKAchievement::initWithIdentifier(GKAchievement::alloc(), &NSString::from_str(id))
    };
    unsafe {
        achievement.setPercentComplete(percent.clamp(0.0, 100.0));
        achievement.setShowsCompletionBanner(true);
    }
    let list = NSArray::from_retained_slice(&[achievement]);
    let report = report.clone();
    let done = RcBlock::new(move |error: *mut NSError| {
        let _ = report.send(finished(request, call, error));
    });
    unsafe {
        GKAchievement::reportAchievements_withCompletionHandler(&list, Some(&done));
    }
}

fn submit_score(request: u64, board: &str, score: i64, report: &Sender<PlatformEvent>) {
    let player = unsafe { GKLocalPlayer::localPlayer() };
    let boards = NSArray::from_retained_slice(&[NSString::from_str(board)]);
    let report = report.clone();
    let done = RcBlock::new(move |error: *mut NSError| {
        let _ = report.send(finished(request, "submit_score", error));
    });
    unsafe {
        GKLeaderboard::submitScore_context_player_leaderboardIDs_completionHandler(
            isize::try_from(score).unwrap_or(isize::MAX),
            0,
            &player,
            &boards,
            &done,
        );
    }
}

fn scores(request: u64, board: &str, count: u32, report: &Sender<PlatformEvent>) {
    let ids = NSArray::from_retained_slice(&[NSString::from_str(board)]);
    let report = report.clone();
    let wanted = board.to_string();
    let loaded = RcBlock::new(
        move |boards: *mut NSArray<GKLeaderboard>, error: *mut NSError| {
            if let Some(error) = unsafe { error.as_ref() } {
                let _ = report.send(PlatformEvent::Failed {
                    request,
                    message: describe(error),
                });
                return;
            }
            let found = unsafe { boards.as_ref() }.and_then(|boards| boards.firstObject());
            let Some(board) = found else {
                let _ = report.send(PlatformEvent::Failed {
                    request,
                    message: format!("no leaderboard {wanted}"),
                });
                return;
            };
            let report = report.clone();
            let wanted = wanted.clone();
            let entries = RcBlock::new(
                move |_local: *mut GKLeaderboardEntry,
                      rows: *mut NSArray<GKLeaderboardEntry>,
                      _total: isize,
                      error: *mut NSError| {
                    let event = if let Some(error) = unsafe { error.as_ref() } {
                        PlatformEvent::Failed {
                            request,
                            message: describe(error),
                        }
                    } else {
                        PlatformEvent::Scores {
                            request,
                            board: wanted.clone(),
                            entries: unsafe { rows.as_ref() }
                                .map(read_entries)
                                .unwrap_or_default(),
                        }
                    };
                    let _ = report.send(event);
                },
            );
            unsafe {
                board.loadEntriesForPlayerScope_timeScope_range_completionHandler(
                    GKLeaderboardPlayerScope::Global,
                    GKLeaderboardTimeScope::AllTime,
                    // Ranks start at 1, and GameKit refuses a range of none.
                    NSRange::new(1, count.max(1) as usize),
                    &entries,
                );
            }
        },
    );
    unsafe {
        GKLeaderboard::loadLeaderboardsWithIDs_completionHandler(Some(&ids), &loaded);
    }
}

fn read_entries(rows: &NSArray<GKLeaderboardEntry>) -> Vec<Score> {
    // By index rather than `iter`, which needs the NSEnumerator feature for
    // nothing this loop cannot do.
    (0..rows.count())
        .map(|at| {
            let entry = rows.objectAtIndex(at);
            let player = unsafe { entry.player() };
            Score {
                player: unsafe { player.gamePlayerID() }.to_string(),
                alias: unsafe { player.alias() }.to_string(),
                rank: unsafe { entry.rank() } as i64,
                score: unsafe { entry.score() } as i64,
            }
        })
        .collect()
}

fn identity(request: u64, report: &Sender<AppleEvent>) {
    let player = unsafe { GKLocalPlayer::localPlayer() };
    if !unsafe { player.isAuthenticated() } {
        let _ = report.send(AppleEvent::Failed {
            request,
            message: "Game Center has no signed-in player to vouch for".into(),
        });
        return;
    }
    let id = unsafe { player.gamePlayerID() }.to_string();
    let report = report.clone();
    let fetched = RcBlock::new(
        move |url: *mut objc2_foundation::NSURL,
              signature: *mut objc2_foundation::NSData,
              salt: *mut objc2_foundation::NSData,
              timestamp: u64,
              error: *mut NSError| {
            let event = if let Some(error) = unsafe { error.as_ref() } {
                AppleEvent::Failed {
                    request,
                    message: describe(error),
                }
            } else {
                AppleEvent::Identity {
                    request,
                    player: id.clone(),
                    url: unsafe { url.as_ref() }
                        .and_then(|url| url.absoluteString())
                        .map(|url| url.to_string())
                        .unwrap_or_default(),
                    // Base64 rather than bytes: what these are for is a JSON
                    // post to the server that checks them.
                    signature: base64(signature),
                    salt: base64(salt),
                    timestamp,
                }
            };
            let _ = report.send(event);
        },
    );
    unsafe {
        player.fetchItemsForIdentityVerificationSignature(Some(&fetched));
    }
}

fn base64(data: *mut objc2_foundation::NSData) -> String {
    unsafe { data.as_ref() }.map_or_else(String::new, |data| {
        data.base64EncodedStringWithOptions(objc2_foundation::NSDataBase64EncodingOptions(0))
            .to_string()
    })
}

fn finished(request: u64, call: &'static str, error: *mut NSError) -> PlatformEvent {
    unsafe { error.as_ref() }.map_or_else(
        || PlatformEvent::Done {
            request,
            call: call.to_string(),
        },
        |error| PlatformEvent::Failed {
            request,
            message: describe(error),
        },
    )
}

fn describe(error: &NSError) -> String {
    error.localizedDescription().to_string()
}

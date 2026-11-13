# The Gamend SDK for Balaur

Written by `clients/sdkgen` (balaur), in Balaur Script. Copy this
directory into a project as `addons/gamend/`, beside the Balaur
library's `addons/save/`, which keeps the session and the prefs; the
editor's Library dock adds both. A script links a module with
`use addons::gamend::<module>;`, beside the engine's own `gamend::login`
and `gamend::rest`:

```rust
use addons::gamend::lobbies;
use addons::gamend::events::lobby;

let reply = await task::wait(lobbies::quick_join({ "title": "duel", "max_users": 2 }));
if e["kind"].as_str() == lobby::MEMBER_JOINED { }
```

285 operations in 44 modules, and 100 realtime events, 15 socket commands.

Beside the generated modules and `events.bs`, written by hand:

| Module | For |
| --- | --- |
| `gamend::client` | `configure`, the socket, hooks and the key-value cache |
| `gamend::auth` | Sign-in through a provider, and the kept session |
| `gamend::presence` | The user cache |
| `gamend::prefs` | The player's prefs on this device |
| `gamend::logs` | This run's log, shipped in batches; put `log_sink.bs` on a node that lives as long as the game |
| `gamend::commands` | Generated: one function per event a channel answers |
| `gamend::core` | The query string, the required-field check, the reply helpers, and the engine's calls, each kept in the activity log |
| `gamend::activity` | The calls `core` sent and what `client::route` heard, for the dock; a call made on `gamend::` directly is not in it |
| `editor/gamend.bs` | The Gamend dock in the Balaur editor: the server, the session, and what the calls answered |

## admin_analytics

Admin – Analytics.

| Function | Call | What it does |
| --- | --- | --- |
| `get_analytics_counts(options)` | `GET /api/v1/admin/analytics/counts` | Daily counters by key or prefix (admin) |
| `get_analytics_daily(options)` | `GET /api/v1/admin/analytics/daily` | Per-day active / new users and cohort retention (admin) |
| `get_analytics_economy(options)` | `GET /api/v1/admin/analytics/economy` | Currency granted / spent per day per ledger reason (admin) |
| `get_analytics_snapshot()` | `GET /api/v1/admin/analytics/snapshot` | Live counters: players, lobbies, parties, quests, matchmaking, tournaments (admin) |
| `get_analytics_summary(options)` | `GET /api/v1/admin/analytics` | DAU / WAU / MAU, D1 / D7 / D30 and payer conversion (admin) |

## admin_bans

Admin – Bans.

| Function | Call | What it does |
| --- | --- | --- |
| `create_ban(params)` | `POST /api/v1/admin/bans` | Ban an IP address, an account or a chat sender (admin) |
| `lift_ban(id)` | `DELETE /api/v1/admin/bans/{id}` | Lift a ban (admin) |
| `list_bans(options)` | `GET /api/v1/admin/bans` | List bans (admin) |

## admin_chat

Admin – Chat.

| Function | Call | What it does |
| --- | --- | --- |
| `create_chat_filter_word(params)` | `POST /api/v1/admin/chat/filter_words` | Add a blocklist word (admin) |
| `delete_chat_conversation(options)` | `DELETE /api/v1/admin/chat/conversation` | Delete all messages in a conversation (admin) |
| `delete_chat_filter_word(id)` | `DELETE /api/v1/admin/chat/filter_words/{id}` | Remove a blocklist word (admin) |
| `delete_chat_filter_words_by_lang(options)` | `DELETE /api/v1/admin/chat/filter_words` | Remove an imported word list (admin) |
| `delete_chat_message(id)` | `DELETE /api/v1/admin/chat/{id}` | Delete a chat message (admin) |
| `delete_chat_report(id)` | `DELETE /api/v1/admin/chat/reports/{id}` | Delete a chat report (admin) |
| `import_chat_filter_words(params)` | `POST /api/v1/admin/chat/filter_words/import` | Import a bundled word list (admin) |
| `list_chat_filter_languages()` | `GET /api/v1/admin/chat/filter_words/languages` | Languages with a bundled word list (admin) |
| `list_chat_filter_words(options)` | `GET /api/v1/admin/chat/filter_words` | List blocklist words (admin) |
| `list_chat_messages(options)` | `GET /api/v1/admin/chat` | List all chat messages (admin) |
| `list_chat_reports(options)` | `GET /api/v1/admin/chat/reports` | List chat reports (admin) |
| `resolve_chat_report(id, params)` | `POST /api/v1/admin/chat/reports/{id}/resolve` | Resolve a chat report (admin) |
| `test_chat_phrase(params)` | `POST /api/v1/admin/chat/filter_words/test` | Test a phrase against the filter (admin) |
| `update_chat_filter_word(id, params)` | `PATCH /api/v1/admin/chat/filter_words/{id}` | Update a blocklist word (admin) |

## admin_economy

Admin – Economy.

| Function | Call | What it does |
| --- | --- | --- |
| `consume_item()` | `POST /api/v1/admin/economy/consume_item` | Consume items from a user (admin) |
| `grant_currency(params)` | `POST /api/v1/admin/economy/grant` | Grant currency to a user (admin) |
| `grant_item(params)` | `POST /api/v1/admin/economy/grant_item` | Grant items to a user (admin) |
| `list_inventory(options)` | `GET /api/v1/admin/economy/items` | List inventory item stacks (admin) |
| `list_ledger(options)` | `GET /api/v1/admin/economy/ledger` | List ledger entries (admin) |
| `list_wallets(options)` | `GET /api/v1/admin/economy/wallets` | List wallets (admin) |
| `spend_currency(params)` | `POST /api/v1/admin/economy/spend` | Spend currency from a user (admin) |

## admin_groups

Admin – Groups.

| Function | Call | What it does |
| --- | --- | --- |
| `delete_group(id)` | `DELETE /api/v1/admin/groups/{id}` | Delete a group (admin) |
| `list_groups(options)` | `GET /api/v1/admin/groups` | List all groups (admin) |
| `update_group(id, params)` | `PATCH /api/v1/admin/groups/{id}` | Update a group (admin) |

## admin_kv

Admin – KV.

| Function | Call | What it does |
| --- | --- | --- |
| `create_kv_entry(params)` | `POST /api/v1/admin/kv/entries` | Create KV entry (admin) |
| `delete_kv(options)` | `DELETE /api/v1/admin/kv` | Delete KV by key (admin) |
| `delete_kv_entry(id)` | `DELETE /api/v1/admin/kv/entries/{id}` | Delete KV entry by id (admin) |
| `list_kv_entries(options)` | `GET /api/v1/admin/kv/entries` | List KV entries (admin) |
| `update_kv_entry(id, params)` | `PATCH /api/v1/admin/kv/entries/{id}` | Update KV entry by id (admin) |
| `upsert_kv(params)` | `PUT /api/v1/admin/kv` | Upsert KV by key (admin) |

## admin_leaderboards

Admin – Leaderboards.

| Function | Call | What it does |
| --- | --- | --- |
| `create_leaderboard(params)` | `POST /api/v1/admin/leaderboards` | Create leaderboard (admin) |
| `delete_leaderboard(id)` | `DELETE /api/v1/admin/leaderboards/{id}` | Delete leaderboard (admin) |
| `delete_leaderboard_record(id, record_id)` | `DELETE /api/v1/admin/leaderboards/{id}/records/{record_id}` | Delete leaderboard record (admin) |
| `delete_leaderboard_user_record(id, user_id)` | `DELETE /api/v1/admin/leaderboards/{id}/records/user/{user_id}` | Delete a user's record (admin) |
| `end_leaderboard(id)` | `POST /api/v1/admin/leaderboards/{id}/end` | End leaderboard (admin) |
| `leaderboard_icon_upload_url(id, params)` | `POST /api/v1/admin/leaderboards/{id}/icon/upload_url` | Request an upload ticket for a leaderboard icon (admin) |
| `set_leaderboard_icon(id, params)` | `POST /api/v1/admin/leaderboards/{id}/icon` | Confirm an uploaded leaderboard icon (admin) |
| `submit_leaderboard_score(id, params)` | `POST /api/v1/admin/leaderboards/{id}/records` | Submit score (admin) |
| `update_leaderboard(id, params)` | `PATCH /api/v1/admin/leaderboards/{id}` | Update leaderboard (admin) |
| `update_leaderboard_record(id, record_id, params)` | `PATCH /api/v1/admin/leaderboards/{id}/records/{record_id}` | Update leaderboard record (admin) |

## admin_lobbies

Admin – Lobbies.

| Function | Call | What it does |
| --- | --- | --- |
| `delete_lobby(id)` | `DELETE /api/v1/admin/lobbies/{id}` | Delete lobby by id (admin) |
| `list_lobbies(options)` | `GET /api/v1/admin/lobbies` | List all lobbies (admin) |
| `update_lobby(id, params)` | `PATCH /api/v1/admin/lobbies/{id}` | Update lobby by id (admin) |

## admin_matchmaking

Admin – Matchmaking.

| Function | Call | What it does |
| --- | --- | --- |
| `cancel_matchmaking_ticket(id)` | `DELETE /api/v1/admin/matchmaking/tickets/{id}` | Cancel a matchmaking ticket (admin) |
| `list_matchmaking_tickets(options)` | `GET /api/v1/admin/matchmaking/tickets` | List matchmaking tickets (admin) |
| `stats()` | `GET /api/v1/admin/matchmaking/stats` | Matchmaking statistics (admin) |

## admin_notifications

Admin – Notifications.

| Function | Call | What it does |
| --- | --- | --- |
| `create_notification(params)` | `POST /api/v1/admin/notifications` | Create a notification (admin) |
| `delete_notification(id)` | `DELETE /api/v1/admin/notifications/{id}` | Delete a notification (admin) |
| `list_notifications(options)` | `GET /api/v1/admin/notifications` | List all notifications (admin) |

## admin_push

Admin – Push.

| Function | Call | What it does |
| --- | --- | --- |
| `delete_push_token(id)` | `DELETE /api/v1/admin/push/tokens/{id}` | Delete a device push token (admin) |
| `list_push_tokens(options)` | `GET /api/v1/admin/push/tokens` | List registered device push tokens (admin) |
| `send_push(params)` | `POST /api/v1/admin/push/send` | Send a push notification to a user (admin) |

## admin_quests

Admin – Quests.

| Function | Call | What it does |
| --- | --- | --- |
| `claim_quest(params)` | `POST /api/v1/admin/quests/claim` | Claim a completed quest on a user's behalf (admin) |
| `create_quest(params)` | `POST /api/v1/admin/quests` | Create quest (admin) |
| `create_quest_code(id, params)` | `POST /api/v1/admin/quests/{id}/codes` | Add one redemption code with a chosen token (admin) |
| `delete_quest(id)` | `DELETE /api/v1/admin/quests/{id}` | Delete quest and all user progress (admin) |
| `generate_quest_codes(id, params)` | `POST /api/v1/admin/quests/{id}/codes/generate` | Generate random redemption codes (admin) |
| `grant_quest(params)` | `POST /api/v1/admin/quests/grant` | Force-complete a quest for a user (admin) |
| `list_quest_codes(id, options)` | `GET /api/v1/admin/quests/{id}/codes` | List a quest's redemption codes (admin) |
| `list_quest_progress(options)` | `GET /api/v1/admin/quests/progress` | List quest progress rows (admin) |
| `list_quests(options)` | `GET /api/v1/admin/quests` | List all quest definitions (admin, includes inactive/hidden) |
| `quest_funnel(key)` | `GET /api/v1/admin/quests/{key}/funnel` | Per-status progress counts for one quest (admin) |
| `quest_icon_upload_url(id, params)` | `POST /api/v1/admin/quests/{id}/icon/upload_url` | Request an upload ticket for a quest icon (admin) |
| `reset_quest(params)` | `POST /api/v1/admin/quests/reset` | Reset a user's current-period quest progress (admin) |
| `set_quest_icon(id, params)` | `POST /api/v1/admin/quests/{id}/icon` | Confirm an uploaded quest icon (admin) |
| `update_quest(id, params)` | `PATCH /api/v1/admin/quests/{id}` | Update quest (admin) |
| `update_quest_code(id, params)` | `PATCH /api/v1/admin/quest_codes/{id}` | Revoke or restore a redemption code (admin) |

## admin_ready_checks

Admin – Ready checks.

| Function | Call | What it does |
| --- | --- | --- |
| `cancel_ready_check(id)` | `DELETE /api/v1/admin/ready_checks/{id}` | Force-cancel a pending ready check (admin) |
| `list_ready_checks(options)` | `GET /api/v1/admin/ready_checks` | List ready checks (admin) |
| `ready_check_stats()` | `GET /api/v1/admin/ready_checks/stats` | Ready check outcomes over the last 24 hours (admin) |

## admin_results

Admin – Results.

| Function | Call | What it does |
| --- | --- | --- |
| `delete_result(id)` | `DELETE /api/v1/admin/results/{id}` | Delete a result and its events (admin) |
| `get_result(id)` | `GET /api/v1/admin/results/{id}` | Get a result (admin) |
| `list_result_events(id, options)` | `GET /api/v1/admin/results/{id}/events` | List a result's events (admin) |
| `list_results(options)` | `GET /api/v1/admin/results` | List results (admin) |

## admin_retention

Admin – Retention.

| Function | Call | What it does |
| --- | --- | --- |
| `get_retention_status()` | `GET /api/v1/admin/retention` | Last retention sweep (admin) |
| `run_retention()` | `POST /api/v1/admin/retention/run` | Run a retention sweep now (admin) |

## admin_sessions

Admin – Sessions.

| Function | Call | What it does |
| --- | --- | --- |
| `delete_session(id)` | `DELETE /api/v1/admin/sessions/{id}` | Delete session token by id (admin) |
| `delete_user_sessions(id)` | `DELETE /api/v1/admin/users/{id}/sessions` | Delete all session tokens for a user (admin) |
| `list_sessions(options)` | `GET /api/v1/admin/sessions` | List sessions (admin) |

## admin_storage

Admin – Storage.

| Function | Call | What it does |
| --- | --- | --- |
| `delete_storage_object(options)` | `DELETE /api/v1/admin/storage` | Delete a stored object (admin) |
| `download_storage_object(options)` | `GET /api/v1/admin/storage/object` | Download an object by key (admin) |
| `list_storage_objects(options)` | `GET /api/v1/admin/storage` | List stored objects with usage (admin) |
| `usage(options)` | `GET /api/v1/admin/storage/usage` | Objects and bytes stored under a prefix (admin) |
| `upload_storage_object(params, options)` | `PUT /api/v1/admin/storage/object` | Upload or overwrite an object at any key (admin) |

## admin_tournaments

Admin – Tournaments.

| Function | Call | What it does |
| --- | --- | --- |
| `cancel_tournament(id)` | `POST /api/v1/admin/tournaments/{id}/cancel` | Cancel tournament (admin; terminal, no recurrence spawn) |
| `create_tournament(params)` | `POST /api/v1/admin/tournaments` | Create tournament (admin) |
| `delete_tournament(id)` | `DELETE /api/v1/admin/tournaments/{id}` | Delete tournament and all its entries/matches (admin) |
| `draw_tournament(id)` | `POST /api/v1/admin/tournaments/{id}/draw` | Draw the bracket now (admin; pulls starts_at to now) |
| `finish_tournament(id)` | `POST /api/v1/admin/tournaments/{id}/finish` | Finish tournament now (admin; pulls ends_at to now) |
| `reopen_tournament(id)` | `POST /api/v1/admin/tournaments/{id}/reopen` | Reopen a cancelled tournament (admin) |
| `resolve_tournament_match(id, match_id, params)` | `POST /api/v1/admin/tournaments/{id}/matches/{match_id}/resolve` | Force a match verdict (admin) |
| `set_tournament_icon(id, params)` | `POST /api/v1/admin/tournaments/{id}/icon` | Confirm an uploaded tournament icon (admin) |
| `tournament_icon_upload_url(id, params)` | `POST /api/v1/admin/tournaments/{id}/icon/upload_url` | Request an upload ticket for a tournament icon (admin) |
| `update_tournament(id, params)` | `PATCH /api/v1/admin/tournaments/{id}` | Update tournament (admin) |

## admin_users

Admin – Users.

| Function | Call | What it does |
| --- | --- | --- |
| `delete_user(id)` | `DELETE /api/v1/admin/users/{id}` | Delete user (admin) |
| `update_user(id, params)` | `PATCH /api/v1/admin/users/{id}` | Update user (admin) |

## authentication

Authentication.

| Function | Call | What it does |
| --- | --- | --- |
| `confirm_registration(params)` | `POST /api/v1/register/confirm` | Confirm a registration with its code |
| `device_login(params)` | `POST /api/v1/login/device` | Device login |
| `link_apple_ios(params)` | `POST /api/v1/me/providers/apple/ios` | Link Apple (native iOS) |
| `link_device(params)` | `POST /api/v1/me/device` | Link device ID |
| `link_google_id_token(params)` | `POST /api/v1/me/providers/google/id_token` | Link Google with an ID token |
| `link_provider(provider, params)` | `POST /api/v1/me/providers/{provider}` | Link a provider with a code |
| `link_provider_request(provider)` | `POST /api/v1/me/providers/{provider}/authorize` | Start linking a provider |
| `link_session_status(session_id)` | `GET /api/v1/me/providers/sessions/{session_id}` | Poll a provider link |
| `list_auth_providers()` | `GET /api/v1/auth/providers` | List sign-in providers |
| `login(params)` | `POST /api/v1/login` | Login |
| `logout()` | `DELETE /api/v1/logout` | Logout |
| `oauth_api_callback(provider, params)` | `POST /api/v1/auth/{provider}/callback` | Sign in with a provider code |
| `oauth_callback_api_apple_ios(params)` | `POST /api/v1/auth/apple/ios/callback` | Sign in with Apple (native iOS) |
| `oauth_google_id_token(params)` | `POST /api/v1/auth/google/id_token` | Sign in with a Google ID token |
| `oauth_request(provider)` | `GET /api/v1/auth/{provider}` | Start a provider sign-in |
| `oauth_session_status(session_id)` | `GET /api/v1/auth/session/{session_id}` | Poll a provider sign-in |
| `refresh_token(params)` | `POST /api/v1/refresh` | Refresh access token |
| `register(params)` | `POST /api/v1/register` | Register |
| `resend_confirmation(params)` | `POST /api/v1/register/resend` | Send the confirmation email again |
| `unlink_device()` | `DELETE /api/v1/me/device` | Unlink device ID |
| `unlink_provider(provider)` | `DELETE /api/v1/me/providers/{provider}` | Unlink OAuth provider |

## chat

Chat.

| Function | Call | What it does |
| --- | --- | --- |
| `unread_count(options)` | `GET /api/v1/chat/unread` | Get unread message count |
| `delete_chat_message(id)` | `DELETE /api/v1/chat/messages/{id}` | Delete your own chat message |
| `get_chat_message(id)` | `GET /api/v1/chat/messages/{id}` | Get a single chat message |
| `list_chat_messages(options)` | `GET /api/v1/chat/messages` | List chat messages |
| `list_group_mutes(id, options)` | `GET /api/v1/groups/{id}/mutes` | List active mutes in a group |
| `list_lobby_mutes(options)` | `GET /api/v1/lobbies/mutes` | List active mutes in your lobby |
| `list_party_mutes(options)` | `GET /api/v1/parties/mutes` | List active mutes in your party |
| `mark_chat_read(params)` | `POST /api/v1/chat/read` | Mark chat as read |
| `mute_group_member(id, params)` | `POST /api/v1/groups/{id}/mute` | Mute a player in a group |
| `mute_lobby_member(params)` | `POST /api/v1/lobbies/mute` | Mute a player in your lobby |
| `mute_party_member(params)` | `POST /api/v1/parties/mute` | Mute a player in your party |
| `report_chat_message(id, params)` | `POST /api/v1/chat/messages/{id}/report` | Report a chat message |
| `send_chat_message(params)` | `POST /api/v1/chat/messages` | Send a chat message |
| `unmute_group_member(id, params)` | `POST /api/v1/groups/{id}/unmute` | Lift a mute in a group |
| `unmute_lobby_member(params)` | `POST /api/v1/lobbies/unmute` | Lift a mute in your lobby |
| `unmute_party_member(params)` | `POST /api/v1/parties/unmute` | Lift a mute in your party |
| `update_chat_message(id, params)` | `PATCH /api/v1/chat/messages/{id}` | Update your own chat message |

## client_logs

Client logs.

| Function | Call | What it does |
| --- | --- | --- |
| `get_client_log_policy()` | `GET /api/v1/client_logs/policy` | Client log capture policy |
| `upload_client_logs(params)` | `POST /api/v1/client_logs` | Upload a batch of client log entries |

## economy

Economy.

| Function | Call | What it does |
| --- | --- | --- |
| `get_current_user_inventory()` | `GET /api/v1/me/inventory` | Current user's item quantities |
| `get_current_user_wallet()` | `GET /api/v1/me/wallet` | Current user's currency balances |
| `list_current_user_ledger(options)` | `GET /api/v1/me/wallet/ledger` | Current user's ledger history |

## friends

Friends.

| Function | Call | What it does |
| --- | --- | --- |
| `accept_friend_request(id)` | `POST /api/v1/friends/{id}/accept` | Accept a friend request |
| `block_friend_request(id)` | `POST /api/v1/friends/{id}/block` | Block a friend request / user |
| `block_user(user_id)` | `POST /api/v1/users/{user_id}/block` | Blacklist a user, with or without an existing friendship |
| `create_friend_request(params)` | `POST /api/v1/friends` | Send a friend request |
| `list_blacklisted_users(options)` | `GET /api/v1/me/blacklist` | List the users you've blocked |
| `list_blocked_friends(options)` | `GET /api/v1/me/blocked` | List users you've blocked |
| `list_friend_requests(options)` | `GET /api/v1/me/friend_requests` | List pending friend requests (incoming and outgoing) |
| `list_friends(options)` | `GET /api/v1/me/friends` | List current user's friends (returns a paginated set of user objects) |
| `reject_friend_request(id)` | `POST /api/v1/friends/{id}/reject` | Reject a friend request |
| `remove_friendship(id)` | `DELETE /api/v1/friends/{id}` | Remove/cancel a friendship or request |
| `unblock_friend(id)` | `POST /api/v1/friends/{id}/unblock` | Unblock a previously-blocked friendship |
| `unblock_user(user_id)` | `POST /api/v1/users/{user_id}/unblock` | Remove a user from your blacklist |

## groups

Groups.

| Function | Call | What it does |
| --- | --- | --- |
| `accept_group_invite(invite_id)` | `POST /api/v1/groups/invitations/{invite_id}/accept` | Accept a group invitation |
| `approve_join_request(id, request_id)` | `POST /api/v1/groups/{id}/join_requests/{request_id}/approve` | Approve a join request (admin only) |
| `cancel_group_invite(invite_id)` | `DELETE /api/v1/groups/sent_invitations/{invite_id}` | Cancel a pending group invitation |
| `cancel_join_request(id, request_id)` | `DELETE /api/v1/groups/{id}/join_requests/{request_id}` | Cancel your own pending join request |
| `create_group(params)` | `POST /api/v1/groups` | Create a group |
| `create_group_icon_upload_url(id, params)` | `POST /api/v1/groups/{id}/icon/upload_url` | Request a group icon upload ticket (admin only) |
| `create_group_invite_link(id)` | `POST /api/v1/groups/{id}/invite_link` | Make a group's invite link (admin only) |
| `decline_group_invite(invite_id)` | `POST /api/v1/groups/invitations/{invite_id}/decline` | Decline a group invitation |
| `delete_group(id)` | `DELETE /api/v1/groups/{id}` | Delete a group (admin only) |
| `delete_group_invite_link(id)` | `DELETE /api/v1/groups/{id}/invite_link` | Turn a group's invite link off (admin only) |
| `demote_group_member(id, params)` | `POST /api/v1/groups/{id}/demote` | Demote admin to member |
| `get_group(id)` | `GET /api/v1/groups/{id}` | Get group details |
| `get_group_invite_link(id)` | `GET /api/v1/groups/{id}/invite_link` | Get a group's invite link (admin only) |
| `invite_to_group(id, params)` | `POST /api/v1/groups/{id}/invite` | Invite a user to a group (admin only) |
| `join_group(id)` | `POST /api/v1/groups/{id}/join` | Join a group |
| `join_group_by_invite_link(token)` | `POST /api/v1/groups/invite_links/{token}/join` | Join a group through its invite link |
| `kick_group_member(id, params)` | `POST /api/v1/groups/{id}/kick` | Kick a member (admin only) |
| `leave_group(id)` | `POST /api/v1/groups/{id}/leave` | Leave a group |
| `list_group_invitations(options)` | `GET /api/v1/groups/invitations` | List my group invitations |
| `list_group_invites(id, options)` | `GET /api/v1/groups/{id}/invites` | List a group's pending invitations (admin only) |
| `list_group_members(id, options)` | `GET /api/v1/groups/{id}/members` | List group members |
| `list_groups(options)` | `GET /api/v1/groups` | List groups |
| `list_join_requests(id, options)` | `GET /api/v1/groups/{id}/join_requests` | List pending join requests (admin only) |
| `list_my_groups(options)` | `GET /api/v1/groups/me` | List groups I belong to |
| `list_sent_invitations(options)` | `GET /api/v1/groups/sent_invitations` | List group invitations I have sent |
| `preview_group_invite_link(token)` | `GET /api/v1/groups/invite_links/{token}` | See the group an invite link opens |
| `promote_group_member(id, params)` | `POST /api/v1/groups/{id}/promote` | Promote member to admin |
| `reject_join_request(id, request_id)` | `POST /api/v1/groups/{id}/join_requests/{request_id}/reject` | Reject a join request (admin only) |
| `set_group_icon(id, params)` | `POST /api/v1/groups/{id}/icon` | Confirm an uploaded group icon (admin only) |
| `update_group(id, params)` | `PATCH /api/v1/groups/{id}` | Update a group (admin only) |

## health

Health.

| Function | Call | What it does |
| --- | --- | --- |
| `index()` | `GET /api/v1/health` | Health check |

## hooks

Hooks.

| Function | Call | What it does |
| --- | --- | --- |
| `call_hook(params)` | `POST /api/v1/hooks/call` | Invoke a hook function |
| `list_hooks(options)` | `GET /api/v1/hooks` | List available hook functions |

## kv

KV.

| Function | Call | What it does |
| --- | --- | --- |
| `get_kv(key, options)` | `GET /api/v1/kv/{key}` | Get a key/value entry |

## leaderboards

Leaderboards.

| Function | Call | What it does |
| --- | --- | --- |
| `get_leaderboard(id)` | `GET /api/v1/leaderboards/{id}` | Get a leaderboard by ID |
| `get_my_record(id, options)` | `GET /api/v1/leaderboards/{id}/records/me` | Get current user's record |
| `list_leaderboard_keys(id, options)` | `GET /api/v1/leaderboards/{id}/keys` | List a leaderboard's rankings |
| `list_leaderboard_records(id, options)` | `GET /api/v1/leaderboards/{id}/records` | List leaderboard records |
| `list_leaderboards(options)` | `GET /api/v1/leaderboards` | List leaderboards |
| `list_records_above(id, options)` | `GET /api/v1/leaderboards/{id}/records/above` | List the records just ahead of a score |
| `list_records_around_user(id, user_id, options)` | `GET /api/v1/leaderboards/{id}/records/around/{user_id}` | List records around a user |
| `resolve_leaderboard_slugs(params)` | `POST /api/v1/leaderboards/resolve` | Resolve multiple slugs to active leaderboards |

## lobbies

Lobbies.

| Function | Call | What it does |
| --- | --- | --- |
| `create_lobby(params)` | `POST /api/v1/lobbies` | Create a lobby |
| `disband_lobby()` | `POST /api/v1/lobbies/disband` | Disband the current lobby (host only) |
| `get_lobby(id)` | `GET /api/v1/lobbies/{id}` | Get a single lobby |
| `join_lobby(id, params)` | `POST /api/v1/lobbies/{id}/join` | Join a lobby |
| `kick_user(params)` | `POST /api/v1/lobbies/kick` | Kick a user from the lobby (host only) |
| `leave_lobby()` | `POST /api/v1/lobbies/leave` | Leave the current lobby |
| `list_lobbies(options)` | `GET /api/v1/lobbies` | List lobbies |
| `lobby_stats()` | `GET /api/v1/lobbies/stats` | Lobby counts |
| `quick_join(params)` | `POST /api/v1/lobbies/quick_join` | Quick-join or create a lobby |
| `set_lobby_state(params)` | `POST /api/v1/lobbies/state` | Set lobby state (host or pinned WebRTC host) |
| `update_lobby(params)` | `PATCH /api/v1/lobbies` | Update lobby (host or pinned WebRTC host) |

## matchmaking

Matchmaking.

| Function | Call | What it does |
| --- | --- | --- |
| `cancel()` | `DELETE /api/v1/matchmaking/tickets` | Leave the matchmaking queue |
| `join(params)` | `POST /api/v1/matchmaking/tickets` | Join the matchmaking queue |
| `my_ticket()` | `GET /api/v1/matchmaking/tickets/me` | Get my current ticket |
| `stats()` | `GET /api/v1/matchmaking/stats` | Queue statistics |

## notifications

Notifications.

| Function | Call | What it does |
| --- | --- | --- |
| `delete_notifications(params)` | `DELETE /api/v1/notifications` | Delete notifications by IDs |
| `list_notifications(options)` | `GET /api/v1/notifications` | List own notifications |
| `send_notification(params)` | `POST /api/v1/notifications` | Send a notification to a friend |

## parties

Parties.

| Function | Call | What it does |
| --- | --- | --- |
| `accept_party_invite(params)` | `POST /api/v1/parties/invite/accept` | Accept a party invite |
| `cancel_party_invite(params)` | `POST /api/v1/parties/invite/cancel` | Cancel a pending party invite (leader only) |
| `create_party(params)` | `POST /api/v1/parties` | Create a party |
| `decline_party_invite(params)` | `POST /api/v1/parties/invite/decline` | Decline a party invite |
| `disband_party()` | `POST /api/v1/parties/disband` | Disband the current party (leader only) |
| `invite_to_party(params)` | `POST /api/v1/parties/invite` | Invite a user to the party (leader only) |
| `kick_party_member(params)` | `POST /api/v1/parties/kick` | Kick a member from the party (leader only) |
| `leave_party()` | `POST /api/v1/parties/leave` | Leave the current party |
| `list_party_invitations(options)` | `GET /api/v1/parties/invitations` | List pending party invites for the current user |
| `list_sent_party_invitations(options)` | `GET /api/v1/parties/invitations/sent` | List pending party invites sent by the current leader |
| `party_create_lobby(params)` | `POST /api/v1/parties/create_lobby` | Create a lobby with the party (leader only) |
| `party_join_lobby(id, params)` | `POST /api/v1/parties/join_lobby/{id}` | Join a lobby with the party (leader only) |
| `party_stats()` | `GET /api/v1/parties/stats` | Party counts |
| `show_party()` | `GET /api/v1/parties/me` | Get current party |
| `update_party(params)` | `PATCH /api/v1/parties` | Update party settings (leader only) |

## payments

Payments.

| Function | Call | What it does |
| --- | --- | --- |
| `apple_webhook(params)` | `POST /api/v1/payments/webhooks/apple` | Receive App Store Server Notification v2 events |
| `catalog(options)` | `GET /api/v1/payments/catalog` | List active payment catalog entries |
| `entitlements(options)` | `GET /api/v1/payments/entitlements` | List current user's active entitlements |
| `google_webhook(params)` | `POST /api/v1/payments/webhooks/google` | Receive Google Play RTDN Pub/Sub push events |
| `steam_checkout(params)` | `POST /api/v1/payments/checkout/steam` | Create a Steam MicroTxn transaction |
| `steam_finalize(params)` | `POST /api/v1/payments/steam/finalize` | Finalize an authorized Steam MicroTxn transaction |
| `stripe_checkout(params)` | `POST /api/v1/payments/checkout/stripe` | Create a Stripe Checkout Session |
| `stripe_webhook(params)` | `POST /api/v1/payments/webhooks/stripe` | Receive Stripe webhook events |
| `validate_store_purchase(provider, params)` | `POST /api/v1/payments/validate/{provider}` | Validate an Apple, Google, or Steam purchase |

## push_tokens

Push.

| Function | Call | What it does |
| --- | --- | --- |
| `delete_push_token(id)` | `DELETE /api/v1/me/push_tokens/{id}` | Unregister one of the current user's devices |
| `list_push_tokens(options)` | `GET /api/v1/me/push_tokens` | List the current user's registered devices |
| `register_push_token(params)` | `POST /api/v1/me/push_tokens` | Register a device push token |

## quests

Quests.

| Function | Call | What it does |
| --- | --- | --- |
| `claim_quest(key)` | `POST /api/v1/me/quests/{key}/claim` | Claim a completed quest |
| `list_quests(options)` | `GET /api/v1/quests` | List quests |
| `list_redeemable_quests(options)` | `GET /api/v1/quests/redeemable` | List quests a code can finish |
| `my_quests(options)` | `GET /api/v1/me/quests` | List my quests |
| `quest_stats()` | `GET /api/v1/quests/stats` | Quest progress counts |
| `redeem_quest_code(params)` | `POST /api/v1/me/quests/redeem` | Redeem a code |
| `user_quests(user_id, options)` | `GET /api/v1/quests/user/{user_id}` | List a user's completed quests |

## ready_checks

Ready checks.

| Function | Call | What it does |
| --- | --- | --- |
| `cancel_lobby_ready_check()` | `DELETE /api/v1/lobbies/ready_check` | Call off the ready check in the caller's lobby (host only) |
| `cancel_party_ready_check()` | `DELETE /api/v1/parties/ready_check` | Call off the ready check in the caller's party (leader only) |
| `get_my_ready_check()` | `GET /api/v1/me/ready_check` | Get the caller's open ready checks |
| `open_lobby_ready_check(params)` | `POST /api/v1/lobbies/ready_check` | Open (or reset) the ready board in the caller's lobby (host only) |
| `open_party_ready_check(params)` | `POST /api/v1/parties/ready_check` | Open (or reset) the ready board in the caller's party (leader only) |
| `respond_ready_check(params)` | `POST /api/v1/me/ready_check` | Answer one of the caller's open ready checks |

## reports

Reports.

| Function | Call | What it does |
| --- | --- | --- |
| `create_report(params)` | `POST /api/v1/reports` | File a report |
| `list_report_kinds()` | `GET /api/v1/reports/kinds` | Report kinds |

## results

Results.

| Function | Call | What it does |
| --- | --- | --- |
| `get_result(id)` | `GET /api/v1/results/{id}` | Get a result |
| `list_result_events(id, options)` | `GET /api/v1/results/{id}/events` | List a result's events |
| `list_results(options)` | `GET /api/v1/results` | List the caller's results |

## signaling

Signaling.

| Function | Call | What it does |
| --- | --- | --- |
| `stats()` | `GET /api/v1/signaling/stats` | WebRTC room counts |

## stats

Stats.

| Function | Call | What it does |
| --- | --- | --- |
| `get_stats()` | `GET /api/v1/stats` | All public server counters in one call |

## time

Time.

| Function | Call | What it does |
| --- | --- | --- |
| `get_server_time()` | `GET /api/v1/time` | Server clock |

## tournaments

Tournaments.

| Function | Call | What it does |
| --- | --- | --- |
| `get_tournament(id)` | `GET /api/v1/tournaments/{id}` | Tournament details (with the caller's participation when authenticated) |
| `join_tournament(id)` | `POST /api/v1/tournaments/{id}/join` | Register as an entry leader |
| `leave_tournament(id)` | `DELETE /api/v1/tournaments/{id}/join` | Withdraw the caller's entry (before the draw) |
| `list_tournaments(options)` | `GET /api/v1/tournaments` | List tournaments |
| `tournament_bracket(id, options)` | `GET /api/v1/tournaments/{id}/bracket` | Brackets and their matches (paginated by bracket) |
| `tournament_entries(id, options)` | `GET /api/v1/tournaments/{id}/entries` | Registered entries (paginated) |
| `tournament_my_match(id)` | `GET /api/v1/tournaments/{id}/my_match` | The caller's current unresolved match |
| `tournament_standings(id)` | `GET /api/v1/tournaments/{id}/standings` | Placements, wins and champions |

## users

Users.

| Function | Call | What it does |
| --- | --- | --- |
| `create_current_user_avatar_upload_url(params)` | `POST /api/v1/me/avatar/upload_url` | Request an avatar upload ticket |
| `delete_current_user(params)` | `DELETE /api/v1/me` | Delete current user |
| `get_current_user()` | `GET /api/v1/me` | Return current user info |
| `get_user(id)` | `GET /api/v1/users/{id}` | Get a user by id |
| `search_users(options)` | `GET /api/v1/users` | Search users by id, username, or display_name |
| `set_current_user_avatar(params)` | `POST /api/v1/me/avatar` | Confirm an uploaded avatar |
| `update_current_user_display_name(params)` | `PATCH /api/v1/me/display_name` | Update current user's display name |
| `update_current_user_password(params)` | `PATCH /api/v1/me/password` | Update current user password |
| `update_current_user_username(params)` | `PATCH /api/v1/me/username` | Update current user's username |
| `user_stats()` | `GET /api/v1/users/stats` | Player counts |

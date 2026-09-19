# The Gamend SDK for Balaur

Written by `clients/generate_balaur.py`. Copy this directory into a
project as `addons/gamend/` and require what you need:

```rune
let api = script::require("addons/gamend/api.rn");
let client = script::require("addons/gamend/client.rn");
```

254 operations and 71 realtime events.

Beside the generated `api.rn` and `events.rn`, written by hand:

| Module | For |
| --- | --- |
| `client.rn` | `configure`, the socket, hooks and the key-value cache |
| `auth.rn` | Sign-in through a provider, and the kept session |
| `presence.rn` | The user cache |
| `prefs.rn` | The player's prefs on this device |
| `logs.rn`, `log_sink.rn` | This run's log, shipped in batches; put `log_sink.rn` on a node that lives as long as the game |
| `editor/gamend.rn` | The Gamend dock in the Balaur editor |

## Admin – Analytics

| Function | Call | What it does |
| --- | --- | --- |
| `admin_analytics_admin_get_analytics_counts(node, options)` | `GET /api/v1/admin/analytics/counts` | Daily counters by key or prefix (admin) |
| `admin_analytics_admin_get_analytics_daily(node, options)` | `GET /api/v1/admin/analytics/daily` | Per-day active / new users and cohort retention (admin) |
| `admin_analytics_admin_get_analytics_economy(node, options)` | `GET /api/v1/admin/analytics/economy` | Currency granted / spent per day per ledger reason (admin) |
| `admin_analytics_admin_get_analytics_snapshot(node)` | `GET /api/v1/admin/analytics/snapshot` | Live counters: players, lobbies, parties, quests, matchmaking, tournaments (admin) |
| `admin_analytics_admin_get_analytics_summary(node)` | `GET /api/v1/admin/analytics` | DAU / WAU / MAU, D1 / D7 / D30 and payer conversion (admin) |

## Admin – Chat

| Function | Call | What it does |
| --- | --- | --- |
| `admin_chat_admin_create_chat_filter_word(node, params)` | `POST /api/v1/admin/chat/filter_words` | Add a blocklist word (admin) |
| `admin_chat_admin_create_chat_mute(node, params)` | `POST /api/v1/admin/chat/mutes` | Mute a player (admin) |
| `admin_chat_admin_delete_chat_conversation(node, options)` | `DELETE /api/v1/admin/chat/conversation` | Delete all messages in a conversation (admin) |
| `admin_chat_admin_delete_chat_filter_word(node, id)` | `DELETE /api/v1/admin/chat/filter_words/{id}` | Remove a blocklist word (admin) |
| `admin_chat_admin_delete_chat_filter_words_by_lang(node, options)` | `DELETE /api/v1/admin/chat/filter_words` | Remove an imported word list (admin) |
| `admin_chat_admin_delete_chat_message(node, id)` | `DELETE /api/v1/admin/chat/{id}` | Delete a chat message (admin) |
| `admin_chat_admin_delete_chat_mute(node, id)` | `DELETE /api/v1/admin/chat/mutes/{id}` | Lift a mute (admin) |
| `admin_chat_admin_delete_chat_report(node, id)` | `DELETE /api/v1/admin/chat/reports/{id}` | Delete a chat report (admin) |
| `admin_chat_admin_import_chat_filter_words(node, params)` | `POST /api/v1/admin/chat/filter_words/import` | Import a bundled word list (admin) |
| `admin_chat_admin_list_chat_filter_languages(node)` | `GET /api/v1/admin/chat/filter_words/languages` | Languages with a bundled word list (admin) |
| `admin_chat_admin_list_chat_filter_words(node, options)` | `GET /api/v1/admin/chat/filter_words` | List blocklist words (admin) |
| `admin_chat_admin_list_chat_messages(node, options)` | `GET /api/v1/admin/chat` | List all chat messages (admin) |
| `admin_chat_admin_list_chat_mutes(node, options)` | `GET /api/v1/admin/chat/mutes` | List chat mutes (admin) |
| `admin_chat_admin_list_chat_reports(node, options)` | `GET /api/v1/admin/chat/reports` | List chat reports (admin) |
| `admin_chat_admin_resolve_chat_report(node, id, params)` | `POST /api/v1/admin/chat/reports/{id}/resolve` | Resolve a chat report (admin) |
| `admin_chat_admin_test_chat_phrase(node, params)` | `POST /api/v1/admin/chat/filter_words/test` | Test a phrase against the filter (admin) |
| `admin_chat_admin_update_chat_filter_word(node, id, params)` | `PATCH /api/v1/admin/chat/filter_words/{id}` | Update a blocklist word (admin) |

## Admin – Economy

| Function | Call | What it does |
| --- | --- | --- |
| `admin_economy_admin_consume_item(node)` | `POST /api/v1/admin/economy/consume_item` | Consume items from a user (admin) |
| `admin_economy_admin_grant_currency(node, params)` | `POST /api/v1/admin/economy/grant` | Grant currency to a user (admin) |
| `admin_economy_admin_grant_item(node, params)` | `POST /api/v1/admin/economy/grant_item` | Grant items to a user (admin) |
| `admin_economy_admin_list_inventory(node, options)` | `GET /api/v1/admin/economy/items` | List inventory item stacks (admin) |
| `admin_economy_admin_list_ledger(node, options)` | `GET /api/v1/admin/economy/ledger` | List ledger entries (admin) |
| `admin_economy_admin_list_wallets(node, options)` | `GET /api/v1/admin/economy/wallets` | List wallets (admin) |
| `admin_economy_admin_spend_currency(node, params)` | `POST /api/v1/admin/economy/spend` | Spend currency from a user (admin) |

## Admin – Groups

| Function | Call | What it does |
| --- | --- | --- |
| `admin_groups_admin_delete_group(node, id)` | `DELETE /api/v1/admin/groups/{id}` | Delete a group (admin) |
| `admin_groups_admin_list_groups(node, options)` | `GET /api/v1/admin/groups` | List all groups (admin) |
| `admin_groups_admin_update_group(node, id, params)` | `PATCH /api/v1/admin/groups/{id}` | Update a group (admin) |

## Admin – KV

| Function | Call | What it does |
| --- | --- | --- |
| `admin_kv_admin_create_kv_entry(node, params)` | `POST /api/v1/admin/kv/entries` | Create KV entry (admin) |
| `admin_kv_admin_delete_kv(node, options)` | `DELETE /api/v1/admin/kv` | Delete KV by key (admin) |
| `admin_kv_admin_delete_kv_entry(node, id)` | `DELETE /api/v1/admin/kv/entries/{id}` | Delete KV entry by id (admin) |
| `admin_kv_admin_list_kv_entries(node, options)` | `GET /api/v1/admin/kv/entries` | List KV entries (admin) |
| `admin_kv_admin_update_kv_entry(node, id, params)` | `PATCH /api/v1/admin/kv/entries/{id}` | Update KV entry by id (admin) |
| `admin_kv_admin_upsert_kv(node, params)` | `PUT /api/v1/admin/kv` | Upsert KV by key (admin) |

## Admin – Leaderboards

| Function | Call | What it does |
| --- | --- | --- |
| `admin_leaderboards_admin_create_leaderboard(node, params)` | `POST /api/v1/admin/leaderboards` | Create leaderboard (admin) |
| `admin_leaderboards_admin_delete_leaderboard(node, id)` | `DELETE /api/v1/admin/leaderboards/{id}` | Delete leaderboard (admin) |
| `admin_leaderboards_admin_delete_leaderboard_record(node, id, record_id)` | `DELETE /api/v1/admin/leaderboards/{id}/records/{record_id}` | Delete leaderboard record (admin) |
| `admin_leaderboards_admin_delete_leaderboard_user_record(node, id, user_id)` | `DELETE /api/v1/admin/leaderboards/{id}/records/user/{user_id}` | Delete a user's record (admin) |
| `admin_leaderboards_admin_end_leaderboard(node, id)` | `POST /api/v1/admin/leaderboards/{id}/end` | End leaderboard (admin) |
| `admin_leaderboards_admin_leaderboard_icon_upload_url(node, id, params)` | `POST /api/v1/admin/leaderboards/{id}/icon/upload_url` | Request an upload ticket for a leaderboard icon (admin) |
| `admin_leaderboards_admin_set_leaderboard_icon(node, id, params)` | `POST /api/v1/admin/leaderboards/{id}/icon` | Confirm an uploaded leaderboard icon (admin) |
| `admin_leaderboards_admin_submit_leaderboard_score(node, id, params)` | `POST /api/v1/admin/leaderboards/{id}/records` | Submit score (admin) |
| `admin_leaderboards_admin_update_leaderboard(node, id, params)` | `PATCH /api/v1/admin/leaderboards/{id}` | Update leaderboard (admin) |
| `admin_leaderboards_admin_update_leaderboard_record(node, id, record_id, params)` | `PATCH /api/v1/admin/leaderboards/{id}/records/{record_id}` | Update leaderboard record (admin) |

## Admin – Lobbies

| Function | Call | What it does |
| --- | --- | --- |
| `admin_lobbies_admin_delete_lobby(node, id)` | `DELETE /api/v1/admin/lobbies/{id}` | Delete lobby by id (admin) |
| `admin_lobbies_admin_list_lobbies(node, options)` | `GET /api/v1/admin/lobbies` | List all lobbies (admin) |
| `admin_lobbies_admin_update_lobby(node, id, params)` | `PATCH /api/v1/admin/lobbies/{id}` | Update lobby by id (admin) |

## Admin – Matchmaking

| Function | Call | What it does |
| --- | --- | --- |
| `admin_matchmaking_admin_cancel_matchmaking_ticket(node, id)` | `DELETE /api/v1/admin/matchmaking/tickets/{id}` | Cancel a matchmaking ticket (admin) |
| `admin_matchmaking_admin_list_matchmaking_tickets(node, options)` | `GET /api/v1/admin/matchmaking/tickets` | List matchmaking tickets (admin) |
| `admin_matchmaking_admin_matchmaking_stats(node)` | `GET /api/v1/admin/matchmaking/stats` | Matchmaking statistics (admin) |

## Admin – Notifications

| Function | Call | What it does |
| --- | --- | --- |
| `admin_notifications_admin_create_notification(node, params)` | `POST /api/v1/admin/notifications` | Create a notification (admin) |
| `admin_notifications_admin_delete_notification(node, id)` | `DELETE /api/v1/admin/notifications/{id}` | Delete a notification (admin) |
| `admin_notifications_admin_list_notifications(node, options)` | `GET /api/v1/admin/notifications` | List all notifications (admin) |

## Admin – Push

| Function | Call | What it does |
| --- | --- | --- |
| `admin_push_admin_delete_push_token(node, id)` | `DELETE /api/v1/admin/push/tokens/{id}` | Delete a device push token (admin) |
| `admin_push_admin_list_push_tokens(node, options)` | `GET /api/v1/admin/push/tokens` | List registered device push tokens (admin) |
| `admin_push_admin_send_push(node, params)` | `POST /api/v1/admin/push/send` | Send a push notification to a user (admin) |

## Admin – Quests

| Function | Call | What it does |
| --- | --- | --- |
| `admin_quests_admin_claim_quest(node, params)` | `POST /api/v1/admin/quests/claim` | Claim a completed quest on a user's behalf (admin) |
| `admin_quests_admin_create_quest(node, params)` | `POST /api/v1/admin/quests` | Create quest (admin) |
| `admin_quests_admin_delete_quest(node, id)` | `DELETE /api/v1/admin/quests/{id}` | Delete quest and all user progress (admin) |
| `admin_quests_admin_grant_quest(node, params)` | `POST /api/v1/admin/quests/grant` | Force-complete a quest for a user (admin) |
| `admin_quests_admin_list_quest_progress(node, options)` | `GET /api/v1/admin/quests/progress` | List quest progress rows (admin) |
| `admin_quests_admin_list_quests(node, options)` | `GET /api/v1/admin/quests` | List all quest definitions (admin, includes inactive/hidden) |
| `admin_quests_admin_quest_funnel(node, key)` | `GET /api/v1/admin/quests/{key}/funnel` | Per-status progress counts for one quest (admin) |
| `admin_quests_admin_quest_icon_upload_url(node, id, params)` | `POST /api/v1/admin/quests/{id}/icon/upload_url` | Request an upload ticket for a quest icon (admin) |
| `admin_quests_admin_reset_quest(node, params)` | `POST /api/v1/admin/quests/reset` | Reset a user's current-period quest progress (admin) |
| `admin_quests_admin_set_quest_icon(node, id, params)` | `POST /api/v1/admin/quests/{id}/icon` | Confirm an uploaded quest icon (admin) |
| `admin_quests_admin_update_quest(node, id, params)` | `PATCH /api/v1/admin/quests/{id}` | Update quest (admin) |

## Admin – Ready checks

| Function | Call | What it does |
| --- | --- | --- |
| `admin_ready_checks_admin_cancel_ready_check(node, id)` | `DELETE /api/v1/admin/ready_checks/{id}` | Force-cancel a pending ready check (admin) |
| `admin_ready_checks_admin_list_ready_checks(node, options)` | `GET /api/v1/admin/ready_checks` | List ready checks (admin) |
| `admin_ready_checks_admin_ready_check_stats(node)` | `GET /api/v1/admin/ready_checks/stats` | Ready check outcomes over the last 24 hours (admin) |

## Admin – Retention

| Function | Call | What it does |
| --- | --- | --- |
| `admin_retention_admin_get_retention_status(node)` | `GET /api/v1/admin/retention` | Last retention sweep (admin) |
| `admin_retention_admin_run_retention(node)` | `POST /api/v1/admin/retention/run` | Run a retention sweep now (admin) |

## Admin – Sessions

| Function | Call | What it does |
| --- | --- | --- |
| `admin_sessions_admin_delete_session(node, id)` | `DELETE /api/v1/admin/sessions/{id}` | Delete session token by id (admin) |
| `admin_sessions_admin_delete_user_sessions(node, id)` | `DELETE /api/v1/admin/users/{id}/sessions` | Delete all session tokens for a user (admin) |
| `admin_sessions_admin_list_sessions(node, options)` | `GET /api/v1/admin/sessions` | List sessions (admin) |

## Admin – Storage

| Function | Call | What it does |
| --- | --- | --- |
| `admin_storage_admin_delete_storage_object(node, options)` | `DELETE /api/v1/admin/storage` | Delete a stored object (admin) |
| `admin_storage_admin_download_storage_object(node, options)` | `GET /api/v1/admin/storage/object` | Download an object by key (admin) |
| `admin_storage_admin_list_storage_objects(node, options)` | `GET /api/v1/admin/storage` | List stored objects with usage (admin) |
| `admin_storage_admin_storage_usage(node, options)` | `GET /api/v1/admin/storage/usage` | Objects and bytes stored under a prefix (admin) |
| `admin_storage_admin_upload_storage_object(node, params, options)` | `PUT /api/v1/admin/storage/object` | Upload or overwrite an object at any key (admin) |

## Admin – Tournaments

| Function | Call | What it does |
| --- | --- | --- |
| `admin_tournaments_admin_cancel_tournament(node, id)` | `POST /api/v1/admin/tournaments/{id}/cancel` | Cancel tournament (admin; terminal, no recurrence spawn) |
| `admin_tournaments_admin_create_tournament(node, params)` | `POST /api/v1/admin/tournaments` | Create tournament (admin) |
| `admin_tournaments_admin_delete_tournament(node, id)` | `DELETE /api/v1/admin/tournaments/{id}` | Delete tournament and all its entries/matches (admin) |
| `admin_tournaments_admin_draw_tournament(node, id)` | `POST /api/v1/admin/tournaments/{id}/draw` | Draw the bracket now (admin; pulls starts_at to now) |
| `admin_tournaments_admin_finish_tournament(node, id)` | `POST /api/v1/admin/tournaments/{id}/finish` | Finish tournament now (admin; pulls ends_at to now) |
| `admin_tournaments_admin_reopen_tournament(node, id)` | `POST /api/v1/admin/tournaments/{id}/reopen` | Reopen a cancelled tournament (admin) |
| `admin_tournaments_admin_resolve_tournament_match(node, id, match_id, params)` | `POST /api/v1/admin/tournaments/{id}/matches/{match_id}/resolve` | Force a match verdict (admin) |
| `admin_tournaments_admin_set_tournament_icon(node, id, params)` | `POST /api/v1/admin/tournaments/{id}/icon` | Confirm an uploaded tournament icon (admin) |
| `admin_tournaments_admin_tournament_icon_upload_url(node, id, params)` | `POST /api/v1/admin/tournaments/{id}/icon/upload_url` | Request an upload ticket for a tournament icon (admin) |
| `admin_tournaments_admin_update_tournament(node, id, params)` | `PATCH /api/v1/admin/tournaments/{id}` | Update tournament (admin) |

## Admin – Users

| Function | Call | What it does |
| --- | --- | --- |
| `admin_users_admin_delete_user(node, id)` | `DELETE /api/v1/admin/users/{id}` | Delete user (admin) |
| `admin_users_admin_update_user(node, id, params)` | `PATCH /api/v1/admin/users/{id}` | Update user (admin) |

## Authentication

| Function | Call | What it does |
| --- | --- | --- |
| `authentication_device_login(node, params)` | `POST /api/v1/login/device` | Device login |
| `authentication_link_device(node, params)` | `POST /api/v1/me/device` | Link device ID |
| `authenticate_list_auth_providers(node)` | `GET /api/v1/auth/providers` | List sign-in providers |
| `authenticate_login(node, params)` | `POST /api/v1/login` | Login |
| `authenticate_logout(node)` | `DELETE /api/v1/logout` | Logout |
| `authenticate_oauth_api_callback(node, provider, params)` | `POST /api/v1/auth/{provider}/callback` | API callback / code exchange |
| `authenticate_oauth_callback_api_apple_ios(node, params)` | `POST /api/v1/auth/apple/ios/callback` | Apple callback (native iOS) |
| `authenticate_oauth_google_id_token(node, params)` | `POST /api/v1/auth/google/id_token` | Google ID token login (native/mobile) |
| `authenticate_oauth_request(node, provider)` | `GET /api/v1/auth/{provider}` | Initiate API OAuth |
| `authenticate_oauth_session_status(node, session_id)` | `GET /api/v1/auth/session/{session_id}` | Get OAuth session status |
| `authentication_refresh_token(node, params)` | `POST /api/v1/refresh` | Refresh access token |
| `authentication_register(node, params)` | `POST /api/v1/register` | Register |
| `authenticate_unlink_device(node)` | `DELETE /api/v1/me/device` | Unlink device ID |
| `authenticate_unlink_provider(node, provider)` | `DELETE /api/v1/me/providers/{provider}` | Unlink OAuth provider |

## Chat

| Function | Call | What it does |
| --- | --- | --- |
| `chat_chat_unread_count(node, options)` | `GET /api/v1/chat/unread` | Get unread message count |
| `chat_delete_chat_message(node, id)` | `DELETE /api/v1/chat/messages/{id}` | Delete your own chat message |
| `chat_get_chat_message(node, id)` | `GET /api/v1/chat/messages/{id}` | Get a single chat message |
| `chat_list_chat_messages(node, options)` | `GET /api/v1/chat/messages` | List chat messages |
| `chat_list_group_mutes(node, id, options)` | `GET /api/v1/groups/{id}/mutes` | List active mutes in a group |
| `chat_list_lobby_mutes(node, options)` | `GET /api/v1/lobbies/mutes` | List active mutes in your lobby |
| `chat_list_party_mutes(node, options)` | `GET /api/v1/parties/mutes` | List active mutes in your party |
| `chat_mark_chat_read(node, params)` | `POST /api/v1/chat/read` | Mark chat as read |
| `chat_mute_group_member(node, id, params)` | `POST /api/v1/groups/{id}/mute` | Mute a player in a group |
| `chat_mute_lobby_member(node, params)` | `POST /api/v1/lobbies/mute` | Mute a player in your lobby |
| `chat_mute_party_member(node, params)` | `POST /api/v1/parties/mute` | Mute a player in your party |
| `chat_report_chat_message(node, id, params)` | `POST /api/v1/chat/messages/{id}/report` | Report a chat message |
| `chat_send_chat_message(node, params)` | `POST /api/v1/chat/messages` | Send a chat message |
| `chat_unmute_group_member(node, id, params)` | `POST /api/v1/groups/{id}/unmute` | Lift a mute in a group |
| `chat_unmute_lobby_member(node, params)` | `POST /api/v1/lobbies/unmute` | Lift a mute in your lobby |
| `chat_unmute_party_member(node, params)` | `POST /api/v1/parties/unmute` | Lift a mute in your party |
| `chat_update_chat_message(node, id, params)` | `PATCH /api/v1/chat/messages/{id}` | Update your own chat message |

## Client logs

| Function | Call | What it does |
| --- | --- | --- |
| `client_logs_get_client_log_policy(node)` | `GET /api/v1/client_logs/policy` | Client log capture policy |
| `client_logs_upload_client_logs(node, params)` | `POST /api/v1/client_logs` | Upload a batch of client log entries |

## Economy

| Function | Call | What it does |
| --- | --- | --- |
| `economy_get_current_user_inventory(node)` | `GET /api/v1/me/inventory` | Current user's item quantities |
| `economy_get_current_user_wallet(node)` | `GET /api/v1/me/wallet` | Current user's currency balances |
| `economy_list_current_user_ledger(node, options)` | `GET /api/v1/me/wallet/ledger` | Current user's ledger history |

## Friends

| Function | Call | What it does |
| --- | --- | --- |
| `friends_accept_friend_request(node, id)` | `POST /api/v1/friends/{id}/accept` | Accept a friend request |
| `friends_block_friend_request(node, id)` | `POST /api/v1/friends/{id}/block` | Block a friend request / user |
| `friends_block_user(node, user_id)` | `POST /api/v1/users/{user_id}/block` | Blacklist a user, with or without an existing friendship |
| `friends_create_friend_request(node, params)` | `POST /api/v1/friends` | Send a friend request |
| `friends_list_blacklisted_users(node, options)` | `GET /api/v1/me/blacklist` | List the users you've blocked |
| `friends_list_blocked_friends(node, options)` | `GET /api/v1/me/blocked` | List users you've blocked |
| `friends_list_friend_requests(node, options)` | `GET /api/v1/me/friend_requests` | List pending friend requests (incoming and outgoing) |
| `friends_list_friends(node, options)` | `GET /api/v1/me/friends` | List current user's friends (returns a paginated set of user objects) |
| `friends_reject_friend_request(node, id)` | `POST /api/v1/friends/{id}/reject` | Reject a friend request |
| `friends_remove_friendship(node, id)` | `DELETE /api/v1/friends/{id}` | Remove/cancel a friendship or request |
| `friends_unblock_friend(node, id)` | `POST /api/v1/friends/{id}/unblock` | Unblock a previously-blocked friendship |
| `friends_unblock_user(node, user_id)` | `POST /api/v1/users/{user_id}/unblock` | Remove a user from your blacklist |

## Groups

| Function | Call | What it does |
| --- | --- | --- |
| `groups_accept_group_invite(node, invite_id)` | `POST /api/v1/groups/invitations/{invite_id}/accept` | Accept a group invitation |
| `groups_approve_join_request(node, id, request_id)` | `POST /api/v1/groups/{id}/join_requests/{request_id}/approve` | Approve a join request (admin only) |
| `groups_cancel_group_invite(node, invite_id)` | `DELETE /api/v1/groups/sent_invitations/{invite_id}` | Cancel a sent group invitation |
| `groups_cancel_join_request(node, id, request_id)` | `DELETE /api/v1/groups/{id}/join_requests/{request_id}` | Cancel your own pending join request |
| `groups_create_group(node, params)` | `POST /api/v1/groups` | Create a group |
| `groups_create_group_icon_upload_url(node, id, params)` | `POST /api/v1/groups/{id}/icon/upload_url` | Request a group icon upload ticket (admin only) |
| `groups_decline_group_invite(node, invite_id)` | `POST /api/v1/groups/invitations/{invite_id}/decline` | Decline a group invitation |
| `groups_demote_group_member(node, id, params)` | `POST /api/v1/groups/{id}/demote` | Demote admin to member |
| `groups_get_group(node, id)` | `GET /api/v1/groups/{id}` | Get group details |
| `groups_invite_to_group(node, id, params)` | `POST /api/v1/groups/{id}/invite` | Invite a user to a group (admin only) |
| `groups_join_group(node, id)` | `POST /api/v1/groups/{id}/join` | Join a group |
| `groups_kick_group_member(node, id, params)` | `POST /api/v1/groups/{id}/kick` | Kick a member (admin only) |
| `groups_leave_group(node, id)` | `POST /api/v1/groups/{id}/leave` | Leave a group |
| `groups_list_group_invitations(node, options)` | `GET /api/v1/groups/invitations` | List my group invitations |
| `groups_list_group_members(node, id, options)` | `GET /api/v1/groups/{id}/members` | List group members |
| `groups_list_groups(node, options)` | `GET /api/v1/groups` | List groups |
| `groups_list_join_requests(node, id, options)` | `GET /api/v1/groups/{id}/join_requests` | List pending join requests (admin only) |
| `groups_list_my_groups(node, options)` | `GET /api/v1/groups/me` | List groups I belong to |
| `groups_list_sent_invitations(node, options)` | `GET /api/v1/groups/sent_invitations` | List group invitations I have sent |
| `groups_promote_group_member(node, id, params)` | `POST /api/v1/groups/{id}/promote` | Promote member to admin |
| `groups_reject_join_request(node, id, request_id)` | `POST /api/v1/groups/{id}/join_requests/{request_id}/reject` | Reject a join request (admin only) |
| `groups_set_group_icon(node, id, params)` | `POST /api/v1/groups/{id}/icon` | Confirm an uploaded group icon (admin only) |
| `groups_update_group(node, id, params)` | `PATCH /api/v1/groups/{id}` | Update a group (admin only) |

## Health

| Function | Call | What it does |
| --- | --- | --- |
| `health_index(node)` | `GET /api/v1/health` | Health check |

## Hooks

| Function | Call | What it does |
| --- | --- | --- |
| `hooks_call_hook(node, params)` | `POST /api/v1/hooks/call` | Invoke a hook function |
| `hooks_list_hooks(node, options)` | `GET /api/v1/hooks` | List available hook functions |

## KV

| Function | Call | What it does |
| --- | --- | --- |
| `kv_get_kv(node, key, options)` | `GET /api/v1/kv/{key}` | Get a key/value entry |

## Leaderboards

| Function | Call | What it does |
| --- | --- | --- |
| `leaderboards_get_leaderboard(node, id)` | `GET /api/v1/leaderboards/{id}` | Get a leaderboard by ID |
| `leaderboards_get_my_record(node, id)` | `GET /api/v1/leaderboards/{id}/records/me` | Get current user's record |
| `leaderboards_list_leaderboard_records(node, id, options)` | `GET /api/v1/leaderboards/{id}/records` | List leaderboard records |
| `leaderboards_list_leaderboards(node, options)` | `GET /api/v1/leaderboards` | List leaderboards |
| `leaderboards_list_records_around_user(node, id, user_id, options)` | `GET /api/v1/leaderboards/{id}/records/around/{user_id}` | List records around a user |
| `leaderboards_resolve_leaderboard_slugs(node, params)` | `POST /api/v1/leaderboards/resolve` | Resolve multiple slugs to active leaderboards |

## Lobbies

| Function | Call | What it does |
| --- | --- | --- |
| `lobbies_create_lobby(node, params)` | `POST /api/v1/lobbies` | Create a lobby |
| `lobbies_disband_lobby(node)` | `POST /api/v1/lobbies/disband` | Disband the current lobby (host only) |
| `lobbies_get_lobby(node, id)` | `GET /api/v1/lobbies/{id}` | Get a single lobby |
| `lobbies_join_lobby(node, id, params)` | `POST /api/v1/lobbies/{id}/join` | Join a lobby |
| `lobbies_kick_user(node, params)` | `POST /api/v1/lobbies/kick` | Kick a user from the lobby (host only) |
| `lobbies_leave_lobby(node)` | `POST /api/v1/lobbies/leave` | Leave the current lobby |
| `lobbies_list_lobbies(node, options)` | `GET /api/v1/lobbies` | List lobbies |
| `lobbies_lobby_stats(node)` | `GET /api/v1/lobbies/stats` | Lobby counts |
| `lobbies_quick_join(node, params)` | `POST /api/v1/lobbies/quick_join` | Quick-join or create a lobby |
| `lobbies_set_lobby_state(node, params)` | `POST /api/v1/lobbies/state` | Set lobby state (host or pinned WebRTC host) |
| `lobbies_update_lobby(node, params)` | `PATCH /api/v1/lobbies` | Update lobby (host or pinned WebRTC host) |

## Matchmaking

| Function | Call | What it does |
| --- | --- | --- |
| `matchmaking_cancel(node)` | `DELETE /api/v1/matchmaking/tickets` | Leave the matchmaking queue |
| `matchmaking_matchmaking_join(node, params)` | `POST /api/v1/matchmaking/tickets` | Join the matchmaking queue |
| `matchmaking_my_ticket(node)` | `GET /api/v1/matchmaking/tickets/me` | Get my current ticket |
| `matchmaking_stats(node)` | `GET /api/v1/matchmaking/stats` | Queue statistics |

## Notifications

| Function | Call | What it does |
| --- | --- | --- |
| `notifications_delete_notifications(node, params)` | `DELETE /api/v1/notifications` | Delete notifications by IDs |
| `notifications_list_notifications(node, options)` | `GET /api/v1/notifications` | List own notifications |
| `notifications_send_notification(node, params)` | `POST /api/v1/notifications` | Send a notification to a friend |

## Parties

| Function | Call | What it does |
| --- | --- | --- |
| `parties_accept_party_invite(node, params)` | `POST /api/v1/parties/invite/accept` | Accept a party invite |
| `parties_cancel_party_invite(node, params)` | `POST /api/v1/parties/invite/cancel` | Cancel a pending party invite (leader only) |
| `parties_create_party(node, params)` | `POST /api/v1/parties` | Create a party |
| `parties_decline_party_invite(node, params)` | `POST /api/v1/parties/invite/decline` | Decline a party invite |
| `parties_disband_party(node)` | `POST /api/v1/parties/disband` | Disband the current party (leader only) |
| `parties_invite_to_party(node, params)` | `POST /api/v1/parties/invite` | Invite a user to the party (leader only) |
| `parties_kick_party_member(node, params)` | `POST /api/v1/parties/kick` | Kick a member from the party (leader only) |
| `parties_leave_party(node)` | `POST /api/v1/parties/leave` | Leave the current party |
| `parties_list_party_invitations(node, options)` | `GET /api/v1/parties/invitations` | List pending party invites for the current user |
| `parties_list_sent_party_invitations(node, options)` | `GET /api/v1/parties/invitations/sent` | List pending party invites sent by the current leader |
| `parties_party_create_lobby(node, params)` | `POST /api/v1/parties/create_lobby` | Create a lobby with the party (leader only) |
| `parties_party_join_lobby(node, id, params)` | `POST /api/v1/parties/join_lobby/{id}` | Join a lobby with the party (leader only) |
| `parties_party_stats(node)` | `GET /api/v1/parties/stats` | Party counts |
| `parties_show_party(node)` | `GET /api/v1/parties/me` | Get current party |
| `parties_update_party(node, params)` | `PATCH /api/v1/parties` | Update party settings (leader only) |

## Payments

| Function | Call | What it does |
| --- | --- | --- |
| `payments_apple_webhook(node, params)` | `POST /api/v1/payments/webhooks/apple` | Receive App Store Server Notification v2 events |
| `payments_catalog(node, options)` | `GET /api/v1/payments/catalog` | List active payment catalog entries |
| `payments_entitlements(node, options)` | `GET /api/v1/payments/entitlements` | List current user's active entitlements |
| `payments_google_webhook(node, params)` | `POST /api/v1/payments/webhooks/google` | Receive Google Play RTDN Pub/Sub push events |
| `payments_steam_checkout(node, params)` | `POST /api/v1/payments/checkout/steam` | Create a Steam MicroTxn transaction |
| `payments_steam_finalize(node, params)` | `POST /api/v1/payments/steam/finalize` | Finalize an authorized Steam MicroTxn transaction |
| `payments_stripe_checkout(node, params)` | `POST /api/v1/payments/checkout/stripe` | Create a Stripe Checkout Session |
| `payments_stripe_webhook(node, params)` | `POST /api/v1/payments/webhooks/stripe` | Receive Stripe webhook events |
| `payments_validate_store_purchase(node, provider, params)` | `POST /api/v1/payments/validate/{provider}` | Validate an Apple, Google, or Steam purchase |

## Push

| Function | Call | What it does |
| --- | --- | --- |
| `push_delete_push_token(node, id)` | `DELETE /api/v1/me/push_tokens/{id}` | Unregister one of the current user's devices |
| `push_list_push_tokens(node, options)` | `GET /api/v1/me/push_tokens` | List the current user's registered devices |
| `push_register_push_token(node, params)` | `POST /api/v1/me/push_tokens` | Register a device push token |

## Quests

| Function | Call | What it does |
| --- | --- | --- |
| `quests_claim_quest(node, key)` | `POST /api/v1/me/quests/{key}/claim` | Claim a completed quest |
| `quests_list_quests(node, options)` | `GET /api/v1/quests` | List quests |
| `quests_my_quests(node, options)` | `GET /api/v1/me/quests` | List my quests |
| `quests_quest_stats(node)` | `GET /api/v1/quests/stats` | Quest progress counts |
| `quests_user_quests(node, user_id, options)` | `GET /api/v1/quests/user/{user_id}` | List a user's completed quests |

## Ready checks

| Function | Call | What it does |
| --- | --- | --- |
| `ready_checks_cancel_lobby(node)` | `DELETE /api/v1/lobbies/ready_check` | Call off the ready check in the caller's lobby (host only) |
| `ready_checks_cancel_party(node)` | `DELETE /api/v1/parties/ready_check` | Call off the ready check in the caller's party (leader only) |
| `ready_checks_get_mine(node)` | `GET /api/v1/me/ready_check` | Get the caller's open ready checks |
| `ready_checks_open_lobby(node, params)` | `POST /api/v1/lobbies/ready_check` | Open (or reset) the ready board in the caller's lobby (host only) |
| `ready_checks_open_party(node, params)` | `POST /api/v1/parties/ready_check` | Open (or reset) the ready board in the caller's party (leader only) |
| `ready_checks_respond_ready_check(node, params)` | `POST /api/v1/me/ready_check` | Answer one of the caller's open ready checks |

## Signaling

| Function | Call | What it does |
| --- | --- | --- |
| `signaling_stats(node)` | `GET /api/v1/signaling/stats` | WebRTC room counts |

## Stats

| Function | Call | What it does |
| --- | --- | --- |
| `stats_get_stats(node)` | `GET /api/v1/stats` | All public server counters in one call |

## Time

| Function | Call | What it does |
| --- | --- | --- |
| `time_get_server_time(node)` | `GET /api/v1/time` | Server clock |

## Tournaments

| Function | Call | What it does |
| --- | --- | --- |
| `tournaments_get_tournament(node, id)` | `GET /api/v1/tournaments/{id}` | Tournament details (with the caller's participation when authenticated) |
| `tournaments_join_tournament(node, id)` | `POST /api/v1/tournaments/{id}/join` | Register as an entry leader |
| `tournaments_leave_tournament(node, id)` | `DELETE /api/v1/tournaments/{id}/join` | Withdraw the caller's entry (before the draw) |
| `tournaments_list_tournaments(node, options)` | `GET /api/v1/tournaments` | List tournaments |
| `tournaments_tournament_bracket(node, id, options)` | `GET /api/v1/tournaments/{id}/bracket` | Brackets and their matches (paginated by bracket) |
| `tournaments_tournament_entries(node, id, options)` | `GET /api/v1/tournaments/{id}/entries` | Registered entries (paginated) |
| `tournaments_tournament_my_match(node, id)` | `GET /api/v1/tournaments/{id}/my_match` | The caller's current unresolved match |
| `tournaments_tournament_standings(node, id)` | `GET /api/v1/tournaments/{id}/standings` | Placements, wins and champions |

## Users

| Function | Call | What it does |
| --- | --- | --- |
| `user_create_current_user_avatar_upload_url(node, params)` | `POST /api/v1/me/avatar/upload_url` | Request an avatar upload ticket |
| `user_delete_current_user(node, params)` | `DELETE /api/v1/me` | Delete current user |
| `users_get_current_user(node)` | `GET /api/v1/me` | Return current user info |
| `users_get_user(node, id)` | `GET /api/v1/users/{id}` | Get a user by id |
| `users_search_users(node, options)` | `GET /api/v1/users` | Search users by id, username, or display_name |
| `user_set_current_user_avatar(node, params)` | `POST /api/v1/me/avatar` | Confirm an uploaded avatar |
| `users_update_current_user_display_name(node, params)` | `PATCH /api/v1/me/display_name` | Update current user's display name |
| `user_update_current_user_password(node, params)` | `PATCH /api/v1/me/password` | Update current user password |
| `users_update_current_user_username(node, params)` | `PATCH /api/v1/me/username` | Update current user's username |
| `user_user_stats(node)` | `GET /api/v1/users/stats` | Player counts |

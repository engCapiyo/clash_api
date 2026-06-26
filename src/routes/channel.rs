use axum::{
    routing::{get, post, put},
    Router,
};

use crate::handlers::channel::{
    add_members_to_channel_handler, approve_join_request_handler, cast_vote_handler,
    check_user_vote_handler, check_user_vote_in_channel_handler, compute_admin_payout_handler,
    compute_admin_reward_score_handler, compute_all_admin_payouts_handler,
    compute_all_admin_reward_scores_handler, create_channel_handler,
    create_pledge_with_vote_handler, finalize_fixture_result_handler,
    get_admin_reward_leaderboard_handler, get_all_channels_handler, get_channel_fixtures_handler,
    get_channel_handler, get_channel_invite_code_handler, get_channel_leaderboard_handler,
    get_fixture_comment_count_handler, get_fixture_latest_comment_handler,
    get_fixture_pledgers_handler, get_fixture_vote_count_handler, get_invite_channel_handler,
    get_messages_handler, get_pending_requests_handler, get_single_fixture_handler,
    get_user_channel_count_handler, get_user_channel_votes_handler, get_user_channels_handler,
    get_user_unread_count_handler, get_user_votes_handler, get_weekly_top_channel_handler,
    initialize_fixture_chat_handler, join_channel_by_code_handler, leave_channel_handler,
    mark_chat_as_read_handler, reject_join_request_handler, request_join_channel_handler,
    reset_weekly_messages_handler,
};
use crate::handlers::ws_handler::ws_comments_handler;
use crate::AppState;

// ============================================================================
// WEBSOCKET ROUTES
// ============================================================================

pub fn ws_channel_routes() -> Router<AppState> {
    Router::new().route("/", get(ws_comments_handler))
}

// ============================================================================
// MAIN CHANNEL ROUTES
// ============================================================================

pub fn channel_routes() -> Router<AppState> {
    Router::new()
        // ====================================================================
        // CHANNEL CRUD
        // ====================================================================
        .route("/create", post(create_channel_handler))
        .route("/:channel_id", get(get_channel_handler))
        .route("/all", get(get_all_channels_handler))
        // Add this with the other routes
        .route("/invite/:code", get(get_invite_channel_handler))
        .route(
            "/:channel_id/admin-payout/compute",
            post(compute_admin_payout_handler).get(compute_admin_payout_handler),
        )
        .route(
            "/admin-payout/compute-all",
            post(compute_all_admin_payouts_handler).get(compute_all_admin_payouts_handler),
        )
        .route(
            "/admin-reward/compute-all",
            post(compute_all_admin_reward_scores_handler)
                .get(compute_all_admin_reward_scores_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/votes/count",
            get(get_fixture_vote_count_handler),
        )
        .route(
            "/:channel_id/invite-code",
            get(get_channel_invite_code_handler),
        )
        // ====================================================================
        // USER CHANNELS
        // ====================================================================
        .route("/user/:user_id", get(get_user_channels_handler))
        .route("/user/:user_id/count", get(get_user_channel_count_handler))
        // ====================================================================
        // CHANNEL MEMBERS
        // ====================================================================
        .route("/members/add", post(add_members_to_channel_handler))
        .route("/members/leave", post(leave_channel_handler))
        // ====================================================================
        // CHANNEL FIXTURES
        // ====================================================================
        .route("/:channel_id/fixtures", get(get_channel_fixtures_handler))
        .route("/fixture/chat", post(initialize_fixture_chat_handler))
        .route(
            "/:channel_id/fixtures/:fixture_id",
            get(get_single_fixture_handler),
        )
        // ====================================================================
        // MESSAGES
        // ====================================================================
        .route("/messages", get(get_messages_handler))
        // ====================================================================
        // JOIN REQUESTS - WITH NOTIFICATIONS
        // ====================================================================
        // User requests to join a channel → sends notification to admins
        .route("/request-join", post(request_join_channel_handler))
        // Admin views pending requests
        .route(
            "/:channel_id/pending-requests",
            get(get_pending_requests_handler),
        )
        // Admin approves join request → sends approval notification to user
        .route("/approve-request", post(approve_join_request_handler))
        // Admin rejects join request → sends rejection notification to user
        .route("/reject-request", post(reject_join_request_handler))
        // Auto-join via invite code (no notification needed)
        .route("/join-by-code", post(join_channel_by_code_handler))
        // ====================================================================
        // PLEDGES (creates a pledge + casts the implied vote atomically)
        // ====================================================================
        // main.rs mounts this router at /api/channels, so this resolves to
        // POST /api/channels/pledges. Flutter's _createPledge calls
        // '$API_BASE_URL/channels/pledges' to match.
        .route("/pledges", post(create_pledge_with_vote_handler))
        .route(
            "/:channel_id/fixtures/:fixture_id/pledgers",
            get(get_fixture_pledgers_handler),
        )
        // ====================================================================
        // VOTES (GLOBAL - no channel_id in path)
        // ====================================================================
        .route("/votes/cast", post(cast_vote_handler))
        .route("/votes/user/:user_id", get(get_user_votes_handler))
        .route(
            "/votes/check/:fixture_id/:user_id",
            get(check_user_vote_handler),
        )
        // ====================================================================
        // CHANNEL VOTES (deprecated - kept for backward compatibility)
        // ====================================================================
        .route(
            "/:channel_id/user/:user_id/votes",
            get(get_user_channel_votes_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/user/:user_id/vote",
            get(check_user_vote_in_channel_handler),
        )
        // ====================================================================
        // FIXTURE FINALIZATION
        // ====================================================================
        .route("/fixtures/finalize", post(finalize_fixture_result_handler))
        // ====================================================================
        // LEADERBOARD & STATS
        // ====================================================================
        .route(
            "/:channel_id/leaderboard",
            get(get_channel_leaderboard_handler),
        )
        .route("/weekly/top", get(get_weekly_top_channel_handler))
        .route("/admin/reset-weekly", post(reset_weekly_messages_handler))
        // ====================================================================
        // ADMIN REWARD SCORING
        // ====================================================================
        // Computes and persists a score for one channel's admin over a
        // rolling window. Call with ?days=7 (or 14/30) — defaults to 7.
        .route(
            "/:channel_id/admin-reward/compute",
            post(compute_admin_reward_score_handler),
        )
        // Ranks all channels' admins by their latest computed score
        .route(
            "/admin-reward/leaderboard",
            get(get_admin_reward_leaderboard_handler),
        )
        // ====================================================================
        // COMMENTS & UNREAD
        // ====================================================================
        .route(
            "/:channel_id/fixtures/:fixture_id/comments/count",
            get(get_fixture_comment_count_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/comments/latest",
            get(get_fixture_latest_comment_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/unread/:user_id",
            get(get_user_unread_count_handler),
        )
        .route(
            "/:channel_id/fixtures/:fixture_id/read/:user_id",
            put(mark_chat_as_read_handler),
        )
}

// ============================================================================
// API DOCUMENTATION FOR JOIN REQUESTS
// ============================================================================

/*
============================================================================
JOIN REQUEST NOTIFICATION FLOW
============================================================================

1. USER REQUESTS TO JOIN
   POST /api/channels/request-join
   Body: {
       "channel_id": "abc123",
       "user_id": "user456",
       "username": "john_doe",
       "user_nickname": "John"  // Optional, for display
   }

   Response: {
       "success": true,
       "message": "Join request sent to admin",
       "pending_requests_count": 3
   }

2. ADMIN RECEIVES PUSH NOTIFICATION
   - Title: "📥 Join Request"
   - Body: "John wants to join 'Football Fans'"
   - Data: {
       "type": "join_request",
       "channel_id": "abc123",
       "channel_name": "Football Fans",
       "user_id": "user456",
       "username": "john_doe",
       "request_id": "user456_abc123"
     }

3. ADMIN VIEWS PENDING REQUESTS
   GET /api/channels/:channel_id/pending-requests

   Response: {
       "success": true,
       "pending_requests": [
           {
               "user_id": "user456",
               "username": "john_doe",
               "requested_at": "2024-01-15T10:30:00Z"
           }
       ],
       "count": 1
   }

4. ADMIN APPROVES
   POST /api/channels/approve-request
   Body: {
       "channel_id": "abc123",
       "user_id": "user456",
       "username": "john_doe"
   }

   Response: {
       "success": true,
       "message": "User approved and added to channel"
   }

   USER RECEIVES NOTIFICATION:
   - Title: "✅ Request Approved!"
   - Body: "You have been added to 'Football Fans' 🎉"
   - Data: {
       "type": "join_approved",
       "channel_id": "abc123",
       "channel_name": "Football Fans",
       "action": "open_channel"
     }

5. ADMIN REJECTS
   POST /api/channels/reject-request
   Body: {
       "channel_id": "abc123",
       "user_id": "user456"
   }

   Response: {
       "success": true,
       "message": "Join request rejected"
   }

   USER RECEIVES NOTIFICATION:
   - Title: "❌ Request Declined"
   - Body: "Your request to join 'Football Fans' was declined"
   - Data: {
       "type": "join_rejected",
       "channel_id": "abc123",
       "channel_name": "Football Fans"
     }

============================================================================
FLUTTER SIDE HANDLING
============================================================================

// In main.dart or notification handler
void handleNotification(Map<String, dynamic> message) {
    final type = message['type'];
    final data = message['data'];

    switch (type) {
        case 'join_request':
            // Admin receives this - show badge on admin dashboard
            _showAdminNotificationBadge();
            break;

        case 'join_approved':
            // User receives this - refresh channel list
            _refreshUserChannels();
            _showSnackBar('You joined ${data['channel_name']}!');
            break;

        case 'join_rejected':
            // User receives this - remove pending state
            _removePendingState(data['channel_id']);
            _showSnackBar('Request to join ${data['channel_name']} was declined');
            break;
    }
}

============================================================================
FCM TOKEN REGISTRATION
============================================================================

// User registers FCM token on login
POST /api/auth/register-token
Body: {
    "user_id": "user456",
    "fcm_token": "fcm_token_here",
    "platform": "android"
}

// When user logs out, remove token
POST /api/auth/remove-token
Body: {
    "user_id": "user456",
    "fcm_token": "fcm_token_here"
}

============================================================================

ADMIN REWARD SCORING
============================================================================

1. COMPUTE SCORE FOR A CHANNEL (run this on a schedule, e.g. nightly cron,
   or trigger manually for testing)
   POST /api/channels/:channel_id/admin-reward/compute?days=7

   Response: {
       "success": true,
       "score": {
           "channel_id": "abc123",
           "admin_user_id": "user789",
           "period_start": "...",
           "period_end": "...",
           "active_member_ratio": 0.62,
           "vote_participation": 0.41,
           "retention_rate": 0.80,
           "net_member_growth": 3,
           "score": 0.51,
           "computed_at": "..."
       }
   }

2. VIEW LEADERBOARD ACROSS ALL CHANNELS
   GET /api/channels/admin-reward/leaderboard

   Response: {
       "success": true,
       "leaderboard": [
           {
               "rank": 1,
               "channel_id": "abc123",
               "admin_user_id": "user789",
               "score": 0.51,
               ...
           }
       ]
   }

   NOTE: leaderboard only reflects channels that have had compute called
   at least once. Wire compute_admin_reward_score_handler into a scheduled
   job per channel (same pattern as reset_weekly_messages_handler) so the
   leaderboard stays fresh without depending on manual/client-triggered calls.

============================================================================
*/

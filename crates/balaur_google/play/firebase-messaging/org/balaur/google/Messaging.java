package org.balaur.google;

import com.google.firebase.messaging.FirebaseMessagingService;
import com.google.firebase.messaging.RemoteMessage;

/**
 * What FCM hands the app while it runs: a replaced token, and a message that
 * reached it in front (or a data message at all). {@link Push} holds both
 * until the game listens. A notification message that arrives while the
 * game is in the background is shown by the system, and reaches the game
 * only when tapped, through the launch intent's extras.
 */
public final class Messaging extends FirebaseMessagingService {
    // 26.0 deprecates it for onRegistered, which answers only an app that opts in to
    // register() with firebase_messaging_installation_id_enabled; this is FCM's default path.
    @SuppressWarnings("deprecation")
    @Override
    public void onNewToken(String token) {
        Push.tokenChanged(token);
    }

    @Override
    public void onMessageReceived(RemoteMessage message) {
        RemoteMessage.Notification shown = message.getNotification();
        Push.received(message.getMessageId(), shown == null ? null : shown.getTitle(),
                shown == null ? null : shown.getBody(), message.getData());
    }
}

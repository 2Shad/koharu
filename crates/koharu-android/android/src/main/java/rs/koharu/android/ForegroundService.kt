package rs.koharu.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat

/**
 * Keeps Koharu's long-running work alive while the app is backgrounded.
 *
 * The caller (the Rust plugin through [KoharuPlugin]) owns the work itself; this service only
 * holds the process priority and an optional progress notification. Every start carries the
 * full notification state, so repeated start/update calls are idempotent.
 */
class ForegroundService : Service() {
  companion object {
    private const val CHANNEL_ID = "koharu-work"
    private const val NOTIFICATION_ID = 1
    private const val ACTION_START = "rs.koharu.android.action.START"
    private const val ACTION_STOP = "rs.koharu.android.action.STOP"
    private const val EXTRA_TITLE = "title"
    private const val EXTRA_TEXT = "text"
    private const val EXTRA_DONE = "done"
    private const val EXTRA_TOTAL = "total"

    /**
     * The running service, if any. Updates go through this reference because background
     * `startForegroundService` calls are rejected on API 31+. Written on the main thread and
     * read from plugin threads.
     */
    @Volatile
    private var instance: ForegroundService? = null

    fun start(context: Context, title: String, text: String, done: Long = -1, total: Long = -1) {
      val intent = Intent(context, ForegroundService::class.java)
        .setAction(ACTION_START)
        .putExtra(EXTRA_TITLE, title)
        .putExtra(EXTRA_TEXT, text)
        .putExtra(EXTRA_DONE, done)
        .putExtra(EXTRA_TOTAL, total)
      context.startForegroundService(intent)
    }

    /** Returns false when the service is not running and the caller must start it. */
    fun update(title: String, text: String, done: Long, total: Long): Boolean {
      val service = instance ?: return false
      service.publish(title, text, done, total)
      return true
    }

    fun stop(context: Context) {
      context.stopService(Intent(context, ForegroundService::class.java))
    }
  }

  private var wakeLock: PowerManager.WakeLock? = null

  override fun onCreate() {
    super.onCreate()
    instance = this
  }

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent == null || intent.action == ACTION_STOP) {
      stopSelf(startId)
      return START_NOT_STICKY
    }

    val title = intent.getStringExtra(EXTRA_TITLE).orEmpty()
    val text = intent.getStringExtra(EXTRA_TEXT).orEmpty()
    val done = intent.getLongExtra(EXTRA_DONE, -1)
    val total = intent.getLongExtra(EXTRA_TOTAL, -1)

    createChannel()
    startForeground(NOTIFICATION_ID, notification(title, text, done, total))
    acquireWakeLock()
    return START_NOT_STICKY
  }

  override fun onDestroy() {
    if (instance === this) instance = null
    wakeLock?.let { if (it.isHeld) it.release() }
    wakeLock = null
    super.onDestroy()
  }

  private fun publish(title: String, text: String, done: Long, total: Long) {
    createChannel()
    val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    manager.notify(NOTIFICATION_ID, notification(title, text, done, total))
  }

  private fun createChannel() {
    val manager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    if (manager.getNotificationChannel(CHANNEL_ID) != null) return
    manager.createNotificationChannel(
      NotificationChannel(
        CHANNEL_ID,
        applicationInfo.loadLabel(packageManager).toString(),
        NotificationManager.IMPORTANCE_LOW,
      ),
    )
  }

  private fun notification(title: String, text: String, done: Long, total: Long): Notification {
    val builder = NotificationCompat.Builder(this, CHANNEL_ID)
      .setContentTitle(title.ifEmpty { applicationInfo.loadLabel(packageManager).toString() })
      .setContentText(text)
      .setSmallIcon(android.R.drawable.stat_sys_download)
      .setOngoing(true)
      .setOnlyAlertOnce(true)
      .setPriority(NotificationCompat.PRIORITY_LOW)
    if (done >= 0 && total > 0) {
      builder.setProgress(total.toInt(), done.toInt(), false)
    } else {
      builder.setProgress(0, 0, true)
    }
    return builder.build()
  }

  private fun acquireWakeLock() {
    if (wakeLock?.isHeld == true) return
    val manager = getSystemService(Context.POWER_SERVICE) as PowerManager
    wakeLock = manager.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "koharu:work").apply {
      setReferenceCounted(false)
      acquire()
    }
  }
}

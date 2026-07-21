package com.ddadan.watchdog

import android.app.ActivityManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.IBinder
import android.util.Log
import com.ddadan.core.CoreConfig
import com.ddadan.core.CoreRepository
import com.ddadan.core.ScreenCapture
import com.ddadan.core.ServerLocator
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.concurrent.atomic.AtomicBoolean

/**
 * 상시 구동 워치독 서비스.
 * - 플레이어(com.ddadan.player)가 죽거나 비포그라운드면 다시 띄운다(OTA 후 자동복귀 해결).
 * - root screencap으로 화면을 캡처해 서버 업로드(동영상 포함).
 * - 자원 텔레메트리를 주기 전송(heartbeat 겸용 → device online 표시).
 */
class WatchdogService : Service() {
  private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
  private lateinit var config: CoreConfig
  private lateinit var repository: CoreRepository
  private val discovering = AtomicBoolean(false)
  private var loopsStarted = false

  override fun onCreate() {
    super.onCreate()
    config = CoreConfig(applicationContext)
    repository = CoreRepository(config)
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    startForegroundCompat()
    if (!loopsStarted) {
      loopsStarted = true
      scope.launch { watchdogLoop() }
      scope.launch { telemetryLoop() }
      scope.launch { captureLoop() }
    }
    return START_STICKY
  }

  // ── 플레이어 감시/재실행 (1~2초 주기) ──────────────────────────────
  private suspend fun watchdogLoop() {
    while (scope.isActive) {
      try {
        if (!isPlayerForeground()) launchPlayer()
      } catch (e: Exception) {
        Log.w(TAG, "watchdog error: ${e.message}")
      }
      delay(WATCHDOG_INTERVAL_MS)
    }
  }

  private fun isPlayerForeground(): Boolean {
    // API 22에서는 getRunningAppProcesses가 전체 목록을 반환한다.
    val am = getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
    val procs = am.runningAppProcesses ?: return false
    for (p in procs) {
      if (p.processName == PLAYER_PACKAGE) {
        return p.importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND
      }
    }
    return false
  }

  private fun launchPlayer() {
    val intent = packageManager.getLaunchIntentForPackage(PLAYER_PACKAGE) ?: return
    intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED)
    try {
      startActivity(intent)
    } catch (e: Exception) {
      Log.w(TAG, "launchPlayer failed: ${e.message}")
    }
  }

  // ── 텔레메트리(heartbeat 겸용) ─────────────────────────────────────
  private suspend fun telemetryLoop() {
    while (scope.isActive) {
      val hwid = config.hardwareId()
      val ok =
        try {
          repository.postTelemetry(hwid, Telemetry.gather(applicationContext))
          true
        } catch (e: Exception) {
          Log.w(TAG, "telemetry failed: ${e.message}")
          false
        }
      if (!ok) discover()
      delay(TELEMETRY_INTERVAL_MS)
    }
  }

  // ── 화면 캡처 업로드 ───────────────────────────────────────────────
  private suspend fun captureLoop() {
    while (scope.isActive) {
      delay(CAPTURE_INTERVAL_MS)
      val hwid = config.hardwareId()
      val jpeg = withContext(Dispatchers.IO) { ScreenCapture.captureJpeg() } ?: continue
      try {
        repository.uploadScreenshot(hwid, jpeg)
      } catch (e: Exception) {
        Log.w(TAG, "screenshot upload failed: ${e.message}")
      }
    }
  }

  /** 서버를 못 찾을 때 LAN 스캔 후 discovered 저장(플레이어와 동일 자동탐색). */
  private suspend fun discover() {
    if (!discovering.compareAndSet(false, true)) return
    try {
      val found = ServerLocator().scanOnce { }
      if (found != null) {
        config.setDiscoveredApiBase(found)
        Log.i(TAG, "discovered server: $found")
      }
    } catch (e: Exception) {
      Log.w(TAG, "discover failed: ${e.message}")
    } finally {
      discovering.set(false)
    }
  }

  private fun startForegroundCompat() {
    val channelId = "ddadan_watchdog"
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      val nm = getSystemService(NotificationManager::class.java)
      if (nm.getNotificationChannel(channelId) == null) {
        nm.createNotificationChannel(
          NotificationChannel(channelId, "DDADAN Agent", NotificationManager.IMPORTANCE_MIN),
        )
      }
    }
    val builder =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        Notification.Builder(this, channelId)
      } else {
        @Suppress("DEPRECATION")
        Notification.Builder(this)
      }
    val notification =
      builder
        .setContentTitle("DDADAN Agent")
        .setContentText("사이니지 감시 중")
        .setSmallIcon(android.R.drawable.ic_menu_manage)
        .build()
    startForeground(NOTIFICATION_ID, notification)
  }

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onDestroy() {
    scope.cancel()
    super.onDestroy()
  }

  companion object {
    private const val TAG = "WatchdogService"
    private const val PLAYER_PACKAGE = "com.ddadan.player"
    private const val NOTIFICATION_ID = 1
    private const val WATCHDOG_INTERVAL_MS = 2_000L
    private const val TELEMETRY_INTERVAL_MS = 20_000L
    private const val CAPTURE_INTERVAL_MS = 30_000L

    fun start(context: Context) {
      val intent = Intent(context, WatchdogService::class.java)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        context.startForegroundService(intent)
      } else {
        context.startService(intent)
      }
    }
  }
}

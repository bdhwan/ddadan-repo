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
import com.ddadan.core.AppUpdater
import com.ddadan.core.CoreConfig
import com.ddadan.core.CoreRepository
import com.ddadan.core.DeviceCommandDto
import com.ddadan.core.RootShell
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
  private lateinit var updater: AppUpdater
  private val discovering = AtomicBoolean(false)
  private var loopsStarted = false
  private var lastDiskCleanupAt = 0L

  override fun onCreate() {
    super.onCreate()
    config = CoreConfig(applicationContext)
    repository = CoreRepository(config)
    updater = AppUpdater(applicationContext, repository)
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    startForegroundCompat()
    if (!loopsStarted) {
      loopsStarted = true
      scope.launch { watchdogLoop() }
      scope.launch { telemetryLoop() }
      scope.launch { commandLoop() }
      scope.launch { otaLoop() }
    }
    return START_STICKY
  }

  // ── 원격 명령 폴링/실행 ───────────────────────────────────────────
  private suspend fun commandLoop() {
    awaitNetwork()
    while (scope.isActive) {
      delay(COMMAND_INTERVAL_MS)
      val hwid = config.hardwareId()
      val commands =
        try {
          repository.getPendingCommands(hwid)
        } catch (e: Exception) {
          emptyList()
        }
      for (cmd in commands) {
        val (status, result) = executeCommand(cmd)
        try {
          repository.ackCommand(hwid, cmd.id, status, result)
        } catch (e: Exception) {
          Log.w(TAG, "ack failed: ${e.message}")
        }
      }
    }
  }

  private suspend fun executeCommand(cmd: DeviceCommandDto): Pair<String, String?> =
    try {
      when (cmd.type) {
        "reboot" -> ok(RootShell.run("reboot"))
        "screenOff" -> ok(RootShell.run("input keyevent 223")) // KEYCODE_SLEEP
        "screenOn" -> ok(RootShell.run("input keyevent 224")) // KEYCODE_WAKEUP
        "screenshot" -> captureAndUpload()
        "shell" -> {
          val out = RootShell.capture(cmd.payload.orEmpty())
          if (out != null) "done" to out.take(4000) else "failed" to null
        }
        "updateApp" -> {
          // 다운로드+설치는 수십 초가 걸린다. commandLoop 은 executeCommand 를 동기로
          // 기다리므로 여기서 붙잡고 있으면 뒤따르는 screenshot/shell/reboot 이 전부
          // pending 에 갇힌다(실제로 박스 2대가 이 상태로 멈췄다). 별도 코루틴에 맡기고
          // 명령은 즉시 ack 한다 — 진행 상황은 OTA 진행 오버레이와 버전 보고로 확인된다.
          val target = cmd.payload?.takeIf { it.isNotBlank() }
          scope.launch {
            if (target != null) {
              updater.updateApp(target)
            } else {
              updater.updateApp(PLAYER_PACKAGE)
              updater.updateApp(packageName)
            }
          }
          "done" to "queued"
        }
        else -> "failed" to "unknown type ${cmd.type}"
      }
    } catch (e: Exception) {
      "failed" to (e.message ?: "error")
    }

  private fun ok(success: Boolean): Pair<String, String?> =
    if (success) "done" to null else "failed" to null

  // ── 주기적 OTA(플레이어 + 워치독 둘 다) ────────────────────────────
  private suspend fun otaLoop() {
    awaitNetwork()
    while (scope.isActive) {
      delay(OTA_INTERVAL_MS)
      try {
        updater.updateApp(PLAYER_PACKAGE)
        updater.updateApp(packageName)
      } catch (e: Exception) {
        Log.w(TAG, "ota loop error: ${e.message}")
      }
    }
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

  /** 네트워크(IPv4 할당)가 준비될 때까지 대기. 부팅 직후 Wi-Fi 전 헛된 서버 접속을 막는다. */
  private suspend fun awaitNetwork() {
    while (scope.isActive && !hasIpv4()) delay(1000)
  }

  private fun hasIpv4(): Boolean =
    try {
      java.net.NetworkInterface.getNetworkInterfaces().toList().any { nif ->
        nif.isUp && !nif.isLoopback &&
          nif.inetAddresses.toList().any {
            it is java.net.Inet4Address && !it.isLoopbackAddress
          }
      }
    } catch (e: Exception) {
      false
    }

  // ── 텔레메트리(heartbeat 겸용) ─────────────────────────────────────
  private suspend fun telemetryLoop() {
    awaitNetwork()
    while (scope.isActive) {
      val hwid = config.hardwareId()
      val telemetry = Telemetry.gather(applicationContext)
      val ok =
        try {
          repository.postTelemetry(hwid, telemetry)
          true
        } catch (e: Exception) {
          Log.w(TAG, "telemetry failed: ${e.message}")
          false
        }
      if (!ok) discover()
      // 매장에 두고 오면 디스크가 차도 사람이 손댈 수 없다. 임계 초과면 스스로 비운다.
      telemetry.diskUsedPercent?.let { maybeFreeDisk(it) }
      delay(TELEMETRY_INTERVAL_MS)
    }
  }

  /**
   * 디스크가 [DISK_CLEANUP_PERCENT] 를 넘으면 캐시를 비운다. 가장 흔한 원인은 플레이어의
   * 이미지 캐시(보드 이미지를 계속 받아 쌓임)와 OTA 잔여 APK.
   *
   * 지우는 대상은 모두 재생성 가능한 것뿐이다 — 캐시/임시 APK. 설정(DataStore)이나 앱
   * 데이터는 건드리지 않으므로 등록정보·서버주소·마지막 화면 캐시는 유지된다.
   * 한 번 돌린 뒤에는 [DISK_CLEANUP_COOLDOWN_MS] 동안 다시 시도하지 않는다(무한 반복 방지).
   */
  private fun maybeFreeDisk(usedPercent: Double) {
    if (usedPercent < DISK_CLEANUP_PERCENT) return
    val now = System.currentTimeMillis()
    if (now - lastDiskCleanupAt < DISK_CLEANUP_COOLDOWN_MS) return
    lastDiskCleanupAt = now
    Log.w(TAG, "disk ${usedPercent}% >= $DISK_CLEANUP_PERCENT% — 캐시 정리 실행")
    RootShell.run(
      // 1) 전체 앱 캐시 회수(pm trim-caches), 2) OTA 스테이징 잔여, 3) 우리 앱 캐시 디렉터리.
      "pm trim-caches 999G; " +
        "rm -f /data/local/tmp/ddadan-*.apk; " +
        "rm -rf /data/data/$PLAYER_PACKAGE/cache/* /data/data/$PLAYER_PACKAGE/code_cache/*; " +
        "rm -rf /data/data/$packageName/cache/* /data/data/$packageName/code_cache/*",
    )
    val after = Telemetry.gather(applicationContext).diskUsedPercent
    Log.i(TAG, "캐시 정리 후 디스크: ${after}%")
  }

  // ── 화면 캡처 업로드 (주기 업로드 제거 — 어드민 원격 요청 시에만) ──────
  private suspend fun captureAndUpload(): Pair<String, String?> {
    val jpeg =
      withContext(Dispatchers.IO) { ScreenCapture.captureJpeg() }
        ?: return "failed" to "capture returned null"
    return try {
      repository.uploadScreenshot(config.hardwareId(), jpeg)
      "done" to null
    } catch (e: Exception) {
      "failed" to (e.message ?: "upload error")
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
    // 하트비트(텔레메트리) 주기. 서버 HEARTBEAT_OFFLINE_AFTER_SECONDS(360s)의 1/3 —
    // 3번 연속 놓쳐야 오프라인 판정. 매장에서 순단에 오프라인으로 깜빡이지 않게.
    private const val TELEMETRY_INTERVAL_MS = 120_000L
    private const val COMMAND_INTERVAL_MS = 10_000L
    private const val OTA_INTERVAL_MS = 10 * 60_000L
    /** 이 사용률(%) 이상이면 워치독이 스스로 캐시를 비운다. */
    private const val DISK_CLEANUP_PERCENT = 85.0
    /** 정리 후 재시도 최소 간격 — 못 비우는 상황에서 매 사이클 반복하지 않도록. */
    private const val DISK_CLEANUP_COOLDOWN_MS = 6 * 60 * 60_000L

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

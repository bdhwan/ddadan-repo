package com.ddadan.core

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * 임의의 대상 앱(플레이어/워치독)을 서버 최신 APK로 OTA 갱신한다(root pm install).
 * **설치 후 다운로드/스테이징 APK를 반드시 삭제**한다(박스 저장공간 부족).
 *
 * - selfPackage=true(자기 자신 갱신): pm install이 이 프로세스를 죽이므로 setsid로 분리 설치 후 재실행.
 * - selfPackage=false(다른 앱 갱신, 예: 워치독→플레이어): 단순 `pm install -r`. 대상 앱은 워치독이
 *   감시 중이라 설치 후 자동 복귀.
 */
class AppUpdater(
  private val context: Context,
  private val repository: CoreRepository,
) {
  private val client =
    OkHttpClient.Builder()
      .connectTimeout(15, TimeUnit.SECONDS)
      .readTimeout(60, TimeUnit.SECONDS)
      .build()

  /** OTA 동시 실행 방지 — otaLoop 과 commandLoop 이 겹쳐 같은 파일을 받는 것을 막는다. */
  private val updateMutex = Mutex()

  /**
   * applicationId 대상 앱을 최신 버전이면 갱신.
   *
   * 호출 경로가 둘이다 — otaLoop(10분 주기)과 commandLoop(admin 의 updateApp 명령). 예전에는
   * 서로를 몰라서 동시에 같은 APK 를 **같은 캐시 파일**에 내려받았다. 진행률이 68% 까지
   * 갔다가 3% 로 되돌아가는 현상이 그것이고(다른 다운로드가 0 부터 덮어씀), 반쯤 겹쳐 쓴
   * APK 는 설치에 실패한다. commandLoop 은 executeCommand 를 동기로 기다리므로 그 명령이
   * 끝나지 않으면 뒤따르는 screenshot/shell 까지 전부 pending 에 갇힌다.
   *
   * 뮤텍스로 한 번에 하나만 돌게 하고, 이미 진행 중이면 조용히 건너뛴다(기다렸다 또 받으면
   * 같은 문제가 반복된다).
   */
  suspend fun updateApp(applicationId: String) {
    if (!updateMutex.tryLock()) {
      Log.i(TAG, "update already in progress; skip $applicationId")
      return
    }
    try {
      updateAppLocked(applicationId)
    } finally {
      updateMutex.unlock()
    }
  }

  private suspend fun updateAppLocked(applicationId: String) {
    val latest =
      try {
        repository.getLatestApk(applicationId)
      } catch (e: Exception) {
        Log.w(TAG, "check failed for $applicationId: ${e.message}")
        return
      }
    if (latest.versionCode <= currentVersionCode(applicationId)) return
    val relUrl = latest.url ?: return
    Log.i(TAG, "updating $applicationId → vc ${latest.versionCode}")

    // 이전 사이클에서 프로세스가 죽어 사후 rm이 안 됐을 수 있는 스테이징 잔여를 시작 시 청소.
    RootShell.run("rm -f /data/local/tmp/ddadan-${applicationId}.apk")

    emitProgress(applicationId, OtaBroadcast.PHASE_DOWNLOADING, 0)
    val apk = File(context.cacheDir, "update-${applicationId}.apk")
    val ok =
      try {
        withContext(Dispatchers.IO) { download(repository.apiOrigin() + relUrl, apk, applicationId) }
      } catch (e: Exception) {
        Log.w(TAG, "download failed: ${e.message}"); false
      }
    if (!ok) {
      emitProgress(applicationId, OtaBroadcast.PHASE_DONE, -1)
      return
    }

    emitProgress(applicationId, OtaBroadcast.PHASE_INSTALLING, 100)
    withContext(Dispatchers.IO) { install(apk, applicationId) }
    // 플레이어 자기 갱신 시엔 이 시점에 프로세스가 죽어 도달 못 할 수 있음(재실행 시 오버레이 사라짐).
    emitProgress(applicationId, OtaBroadcast.PHASE_DONE, -1)
  }

  /** 진행률을 플레이어로 브로드캐스트(좌상단 오버레이 표시용). 실패는 무시. */
  private fun emitProgress(app: String, phase: String, percent: Int) {
    try {
      context.sendBroadcast(
        Intent(OtaBroadcast.ACTION)
          .setPackage(OtaBroadcast.PLAYER_PACKAGE)
          .putExtra(OtaBroadcast.EXTRA_APP, app)
          .putExtra(OtaBroadcast.EXTRA_PHASE, phase)
          .putExtra(OtaBroadcast.EXTRA_PERCENT, percent),
      )
    } catch (_: Exception) {
    }
  }

  private fun currentVersionCode(applicationId: String): Int =
    try {
      @Suppress("DEPRECATION")
      context.packageManager.getPackageInfo(applicationId, 0).versionCode
    } catch (_: PackageManager.NameNotFoundException) {
      0 // 미설치면 0 → 항상 설치
    }

  private fun download(url: String, dest: File, applicationId: String): Boolean {
    client.newCall(Request.Builder().url(url).get().build()).execute().use { resp ->
      if (!resp.isSuccessful) return false
      val body = resp.body ?: return false
      val total = body.contentLength()
      dest.outputStream().use { out ->
        val input = body.byteStream()
        val buf = ByteArray(64 * 1024)
        var copied = 0L
        var lastPct = -1
        while (true) {
          val n = input.read(buf)
          if (n < 0) break
          out.write(buf, 0, n)
          copied += n
          if (total > 0) {
            val pct = (copied * 100 / total).toInt()
            if (pct != lastPct) {
              lastPct = pct
              emitProgress(applicationId, OtaBroadcast.PHASE_DOWNLOADING, pct)
            }
          }
        }
      }
    }
    return dest.length() > 0
  }

  /**
   * 포그라운드 su로 설치. 대상 앱(플레이어/워치독)은 설치로 죽지만, **상대 앱이 상호 감시로 재실행**한다
   * (워치독→플레이어, 플레이어→워치독). 포그라운드 설치는 앱이 죽어도 system_server가 끝까지 완료하므로
   * 확실하다(취약한 detached 재실행/재부팅 불필요). 설치 후 캐시·스테이징 APK 삭제(저장공간 부족).
   */
  private fun install(apk: File, applicationId: String) {
    val staged = "/data/local/tmp/ddadan-${applicationId}.apk"
    // 1) 스테이징(앱 살아있음) 후 캐시 APK 즉시 삭제.
    RootShell.run("cp '${apk.absolutePath}' $staged; chmod 644 $staged")
    apk.delete()
    // 2) 설치(설치가 이 프로세스를 죽일 수 있어 사후 rm은 best-effort; 잔여는 다음 사이클 시작 시 청소).
    RootShell.run(
      "setprop dalvik.vm.dex2oat-filter interpret-only; pm install -r $staged; rm -f $staged",
    )
  }

  companion object {
    private const val TAG = "AppUpdater"
  }
}

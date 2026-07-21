package com.ddadan.core

import android.content.Context
import android.content.pm.PackageManager
import android.util.Log
import kotlinx.coroutines.Dispatchers
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

  /** applicationId 대상 앱을 최신 버전이면 갱신. */
  suspend fun updateApp(applicationId: String) {
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

    val apk = File(context.cacheDir, "update-${applicationId}.apk")
    val ok =
      try {
        withContext(Dispatchers.IO) { download(repository.apiOrigin() + relUrl, apk) }
      } catch (e: Exception) {
        Log.w(TAG, "download failed: ${e.message}"); false
      }
    if (!ok) return

    withContext(Dispatchers.IO) { install(apk, applicationId) }
  }

  private fun currentVersionCode(applicationId: String): Int =
    try {
      @Suppress("DEPRECATION")
      context.packageManager.getPackageInfo(applicationId, 0).versionCode
    } catch (_: PackageManager.NameNotFoundException) {
      0 // 미설치면 0 → 항상 설치
    }

  private fun download(url: String, dest: File): Boolean {
    client.newCall(Request.Builder().url(url).get().build()).execute().use { resp ->
      if (!resp.isSuccessful) return false
      val body = resp.body ?: return false
      dest.outputStream().use { out -> body.byteStream().copyTo(out) }
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

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

    val apk = File(context.cacheDir, "update-${applicationId}.apk")
    val ok =
      try {
        withContext(Dispatchers.IO) { download(repository.apiOrigin() + relUrl, apk) }
      } catch (e: Exception) {
        Log.w(TAG, "download failed: ${e.message}"); false
      }
    if (!ok) return

    val selfPackage = applicationId == context.packageName
    withContext(Dispatchers.IO) { install(apk, applicationId, selfPackage) }
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

  private fun install(apk: File, applicationId: String, selfPackage: Boolean) {
    val staged = "/data/local/tmp/ddadan-${applicationId}.apk"
    if (selfPackage) {
      // 자기 자신: setsid 분리 스크립트로 설치+재실행+APK삭제.
      val activity = "$applicationId/.MainActivity"
      val script =
        buildString {
          append("chmod 644 $staged\n")
          append("setprop dalvik.vm.dex2oat-filter interpret-only\n")
          append("pm install -r $staged\n")
          append("rm -f $staged\n")
          append("sleep 6\n")
          append("am start -n $activity\n")
        }
      val scriptFile = File(apk.parentFile, "ota-${applicationId}.sh")
      scriptFile.writeText(script)
      RootShell.run(
        "cp '${apk.absolutePath}' $staged; cp '${scriptFile.absolutePath}' " +
          "/data/local/tmp/ota-${applicationId}.sh; chmod 644 $staged; " +
          "chmod 755 /data/local/tmp/ota-${applicationId}.sh; " +
          "setsid sh /data/local/tmp/ota-${applicationId}.sh </dev/null >/dev/null 2>&1",
      )
    } else {
      // 다른 앱: 단순 설치(대상은 워치독이 재실행). 설치 후 스테이징 APK 삭제.
      RootShell.run(
        "cp '${apk.absolutePath}' $staged; chmod 644 $staged; " +
          "setprop dalvik.vm.dex2oat-filter interpret-only; " +
          "pm install -r $staged; rm -f $staged",
      )
    }
    // 다운로드 캐시 APK 삭제(자기 자신도 설치는 스테이징본으로 진행되므로 캐시는 지워도 됨).
    apk.delete()
  }

  companion object {
    private const val TAG = "AppUpdater"
  }
}

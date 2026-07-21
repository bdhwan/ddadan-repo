package com.ddadan.player.data

import android.content.Context
import android.util.Log
import com.ddadan.player.BuildConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * API 서버에 올라온 최신 APK를 확인해, 현재 설치본보다 versionCode가 높으면 내려받아 설치한다.
 *
 * 설치는 root(su)로 무인 진행하며, 우리 앱의 큰 dex 때문에 박스 dex2oat가 죽는 문제를 피하려고
 * 설치 직전 `dex2oat-filter interpret-only`를 세팅한다. su가 없으면 시스템 설치 UI로 폴백한다.
 */
class AppUpdater(
  private val context: Context,
  private val repository: PlayerRepository,
) {
  private val client =
    OkHttpClient.Builder()
      .connectTimeout(15, TimeUnit.SECONDS)
      .readTimeout(60, TimeUnit.SECONDS)
      .build()

  suspend fun checkAndUpdate() {
    val latest =
      try {
        repository.getLatestApk()
      } catch (e: Exception) {
        Log.w(TAG, "update check failed: ${e.message}")
        return
      }
    if (latest.versionCode <= BuildConfig.VERSION_CODE) return
    val relUrl = latest.url ?: return
    Log.i(TAG, "new version ${latest.versionCode} (current ${BuildConfig.VERSION_CODE})")

    val fileUrl = repository.apiOrigin() + relUrl
    val apk = File(context.cacheDir, "ddadan-update.apk")
    val downloaded =
      try {
        withContext(Dispatchers.IO) { download(fileUrl, apk) }
      } catch (e: Exception) {
        Log.w(TAG, "download failed: ${e.message}")
        false
      }
    if (!downloaded) return

    val installed = withContext(Dispatchers.IO) { installWithRoot(apk) }
    if (!installed) {
      // 사이니지 박스는 root가 보장되므로 여기 도달은 드물다(su 부재 등). 시스템 설치 UI는
      // 무인 운영에서 사용자 탭이 필요하고 app-private 캐시 파일을 못 읽어 실패하므로 쓰지 않는다.
      Log.w(TAG, "root install could not be launched (su unavailable?)")
    }
  }

  private fun download(url: String, dest: File): Boolean {
    val request = Request.Builder().url(url).get().build()
    client.newCall(request).execute().use { resp ->
      if (!resp.isSuccessful) return false
      val body = resp.body ?: return false
      dest.outputStream().use { out -> body.byteStream().copyTo(out) }
    }
    return dest.length() > 0
  }

  /**
   * root로 무인 설치 + 설치 후 자동 재실행.
   *
   * 앱 캐시 파일(mode 600)은 시스템이 못 읽으므로 /data/local/tmp 로 복사 + chmod 644 후 설치한다.
   * 설치가 끝나면 다운로드 APK를 남기지 않도록 캐시본과 스테이징본을 모두 삭제한다.
   */
  private fun installWithRoot(apk: File): Boolean {
    val stagedApk = "/data/local/tmp/ddadan-update.apk"
    val activity = "${BuildConfig.APPLICATION_ID}/com.ddadan.player.MainActivity"

    // 1) APK 스테이징: 앱 캐시(mode 600)는 시스템이 못 읽으므로 /data/local/tmp 로 복사 + 644.
    val staged = runSu("cp '${apk.absolutePath}' $stagedApk; chmod 644 $stagedApk")
    if (!staged) return false

    // 다운로드 캐시 APK는 이미 tmp로 복사됐으니 즉시 삭제(앱이 살아있는 지금 확실히 지운다).
    apk.delete()

    // 2) 재실행 예약(best-effort): 설치가 앱을 죽이므로 분리된 root 프로세스가 잠시 뒤 앱을 다시
    //    띄운다. 아울러 설치가 끝난 뒤 스테이징 APK도 정리한다(설치 커맨드가 앱과 함께 죽어
    //    rm 을 못 돌릴 수 있으므로 여기서도 지운다).
    runSu(
      "setsid sh -c \"sleep 14; rm -f $stagedApk; am start -n $activity; sleep 4; am start -n $activity\" " +
        "</dev/null >/dev/null 2>&1 &",
    )

    // 3) 설치: 포그라운드 su로 실행하면 앱이 죽더라도 system_server가 설치를 끝까지 완료한다.
    //    dex2oat OOM을 피하려 interpret-only 로 둔다(R8로 dex가 작아 부담 없음).
    runSu("setprop dalvik.vm.dex2oat-filter interpret-only; pm install -r $stagedApk; rm -f $stagedApk")
    return true
  }

  private fun runSu(cmd: String): Boolean =
    try {
      val process = Runtime.getRuntime().exec(arrayOf("su", "-c", cmd))
      process.waitFor() == 0
    } catch (e: Exception) {
      Log.w(TAG, "su failed: ${e.message}")
      false
    }

  companion object {
    private const val TAG = "AppUpdater"
  }
}

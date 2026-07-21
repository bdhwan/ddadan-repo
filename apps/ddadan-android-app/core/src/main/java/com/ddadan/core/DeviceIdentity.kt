package com.ddadan.core

import android.annotation.SuppressLint
import android.content.Context
import android.provider.Settings

/**
 * 박스별 고유·안정 하드웨어 식별자.
 *
 * 같은 박스의 플레이어·워치독 두 앱이 **동일한 값**을 계산해야 서버에서 한 device로 합쳐진다
 * (패키지가 달라 저장소를 공유할 수 없으므로 저장 대신 계산으로 맞춘다).
 * 1순위 `ro.serialno`(하드웨어 시리얼, root로 조회), 2순위 `ANDROID_ID`(API 22는 기기 단위).
 */
object DeviceIdentity {
  @SuppressLint("HardwareIds")
  fun hardwareId(context: Context): String {
    RootShell.capture("getprop ro.serialno")?.trim()?.takeIf { it.isNotBlank() }?.let {
      return sanitize(it)
    }
    val androidId =
      Settings.Secure.getString(context.contentResolver, Settings.Secure.ANDROID_ID)
    if (!androidId.isNullOrBlank()) return sanitize(androidId)
    return "unknown-device"
  }

  private fun sanitize(raw: String): String =
    raw.filter { it.isLetterOrDigit() || it == '-' || it == '_' }.take(64)
}

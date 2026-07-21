package com.ddadan.core

import android.util.Log
import java.io.BufferedReader
import java.io.InputStreamReader

/** root(su) 셸 실행 헬퍼. 박스는 root가 있으므로 screencap/reboot/pm install 등에 사용. */
object RootShell {
  private const val TAG = "RootShell"

  /** cmd를 `su -c`로 실행하고 종료코드 0이면 true. */
  fun run(cmd: String): Boolean =
    try {
      Runtime.getRuntime().exec(arrayOf("su", "-c", cmd)).waitFor() == 0
    } catch (e: Exception) {
      Log.w(TAG, "run failed: ${e.message}")
      false
    }

  /** cmd를 실행하고 표준출력을 문자열로 반환(실패 시 null). */
  fun capture(cmd: String): String? =
    try {
      val process = Runtime.getRuntime().exec(arrayOf("su", "-c", cmd))
      val out = BufferedReader(InputStreamReader(process.inputStream)).readText()
      process.waitFor()
      out
    } catch (e: Exception) {
      Log.w(TAG, "capture failed: ${e.message}")
      null
    }

  /** su 사용 가능 여부. */
  fun isAvailable(): Boolean = run("id")
}

package com.ddadan.core

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Log
import java.io.ByteArrayOutputStream

/**
 * root `screencap`로 현재 화면 전체를 캡처한다. `View.draw`와 달리 하드웨어 영상 레이어(동영상)까지
 * 포함된다. 크라이저 Monitor와 동일하게 대화형 su에 `screencap -p`를 써 PNG 스트림을 디코드한다.
 */
object ScreenCapture {
  private const val TAG = "ScreenCapture"

  fun captureJpeg(quality: Int = 60): ByteArray? =
    try {
      val process = Runtime.getRuntime().exec("su")
      val os = process.outputStream
      os.write("screencap -p\n".toByteArray())
      os.flush()
      val bitmap: Bitmap? = BitmapFactory.decodeStream(process.inputStream)
      os.write("exit\n".toByteArray())
      os.flush()
      process.waitFor()
      if (bitmap == null) {
        null
      } else {
        val out = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.JPEG, quality, out)
        bitmap.recycle()
        out.toByteArray()
      }
    } catch (e: Throwable) {
      Log.w(TAG, "capture failed: ${e.message}")
      null
    }
}

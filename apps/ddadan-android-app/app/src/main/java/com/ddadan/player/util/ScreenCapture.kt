package com.ddadan.player.util

import android.graphics.Bitmap
import android.graphics.Canvas
import android.view.View
import java.io.ByteArrayOutputStream

/**
 * 현재 화면(Compose 루트 View)을 JPEG 바이트로 캡처한다.
 *
 * Android 5.1(API 22)에서는 PixelCopy(24+)를 못 쓰므로 `View.draw(Canvas)`로 소프트웨어
 * 렌더한다. 반드시 UI 스레드에서 호출해야 한다. 하드웨어 SurfaceView(동영상)는 검게 나올 수 있다.
 */
fun captureViewToJpeg(view: View, quality: Int = 60): ByteArray? {
  val w = view.width
  val h = view.height
  if (w <= 0 || h <= 0) return null
  return try {
    val bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
    val canvas = Canvas(bitmap)
    view.draw(canvas)
    val out = ByteArrayOutputStream()
    bitmap.compress(Bitmap.CompressFormat.JPEG, quality, out)
    bitmap.recycle()
    out.toByteArray()
  } catch (_: Throwable) {
    null
  }
}

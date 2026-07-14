package com.ddadan.player.util

import com.ddadan.player.data.ScreenItem
import com.ddadan.player.data.SlidePayload

fun resolveFontSizeSp(item: ScreenItem, stageHeightPx: Float): Float {
  val fontSize = item.fontSize ?: return 16f
  return when (item.fontUnit) {
    "vh" -> (fontSize * stageHeightPx / 100.0).toFloat()
    else -> fontSize.toFloat()
  }
}

fun buildRotationKey(slides: List<SlidePayload>, intervalMs: Long, fadeMs: Long): String {
  val slideKeys = slides.joinToString("||") { slide ->
    slide.items.joinToString(",") { it.id }
  }
  return "${slides.size}|$intervalMs|$fadeMs|$slideKeys"
}

package com.ddadan.player.util

import com.ddadan.player.data.ScreenItem
import com.ddadan.player.data.SlidePayload

/**
 * Font size in Dp — the same unit space [com.ddadan.player.ui.ScreenStage] lays item
 * boxes out in. Both units resolve to a fraction of the stage height, so text keeps its
 * authored proportion to its box on any panel:
 *
 *   vh       — percent of stage height, as in CSS on the web player.
 *   px (null)— a length in the board's design space (typically 1920x1080), the same space
 *              item.height is in. It must be scaled by stageHeight/designHeight exactly
 *              like the boxes are, or a 60px caption authored inside an 80px box stops
 *              fitting the moment the stage is not 1080 tall.
 *
 * The result must NOT be passed to `.sp` directly. `.sp` multiplies by display density
 * and the user's font-scale setting, so on this 2x-density tablet every board rendered
 * its text at double size and clipped it out of its box. Convert with `Density.toSp()`
 * so the rendered pixel size is exactly what the layout expects.
 */
fun resolveFontSizeDp(item: ScreenItem, stageHeightDp: Float, designHeight: Int): Float {
  val fontSize = item.fontSize ?: return 16f
  val stageFraction =
    when (item.fontUnit) {
      "vh" -> fontSize / 100.0
      else -> fontSize / designHeight.coerceAtLeast(1).toDouble()
    }
  return (stageFraction * stageHeightDp).toFloat()
}

fun buildRotationKey(slides: List<SlidePayload>, intervalMs: Long, fadeMs: Long): String {
  val slideKeys = slides.joinToString("||") { slide ->
    slide.items.joinToString(",") { it.id }
  }
  return "${slides.size}|$intervalMs|$fadeMs|$slideKeys"
}

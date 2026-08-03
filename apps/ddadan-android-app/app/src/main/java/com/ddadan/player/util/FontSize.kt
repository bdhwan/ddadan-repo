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

/**
 * 로테이션 갱신 감지 키. 이 값이 바뀌어야 플레이어가 새 슬라이드를 화면에 반영한다.
 *
 * 예전에는 아이템의 `id` 만 이어붙였다. 그래서 슬라이드 구성(개수·id)이 그대로면 가격이나
 * 문구를 고쳐도 키가 같아 화면이 옛 내용을 계속 렌더했다 — 메뉴를 바꿀 때마다 플레이어를
 * 재시작해야 했던 원인. 실제로 가격만 바꾸거나(7,000→6,000) 영문명만 줄인 경우
 * id 는 그대로였다.
 *
 * ScreenItem/SlidePayload 는 data class 라 toString() 이 모든 필드를 포함한다. 그걸
 * 해시하면 어떤 필드가 바뀌든(가격·문구·색·좌표·뱃지…) 키가 달라지고, 필드가 늘어도
 * 여기를 따라 고칠 필요가 없다. 문자열 전체 대신 hashCode 만 담아 키를 짧게 유지한다.
 */
fun buildRotationKey(slides: List<SlidePayload>, intervalMs: Long, fadeMs: Long): String {
  val contentHash = slides.joinToString("||") { slide ->
    "${slide.width}x${slide.height}:${slide.background}:" +
      slide.items.joinToString(",") { it.toString().hashCode().toString(16) }
  }.hashCode().toString(16)
  return "${slides.size}|$intervalMs|$fadeMs|$contentHash"
}

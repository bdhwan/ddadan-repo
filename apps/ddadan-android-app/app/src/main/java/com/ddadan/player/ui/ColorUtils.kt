package com.ddadan.player.ui

import androidx.compose.ui.graphics.Color

/**
 * 색 문자열을 Compose Color로 파싱한다. 서버/웹(CSS)과 스키마를 공유하므로 hex(#rrggbb,
 * #aarrggbb)뿐 아니라 `rgb(r,g,b)` / `rgba(r,g,b,a)` 도 지원해야 한다 — 그룹 카드·안내박스·
 * 구분선 틴트가 모두 rgba로 저작되기 때문. (android.graphics.Color.parseColor는 rgba 미지원.)
 */
fun parseColorOrNull(value: String?): Color? {
  if (value.isNullOrBlank()) return null
  val s = value.trim()
  return try {
    when {
      s.startsWith("rgba(", ignoreCase = true) || s.startsWith("rgb(", ignoreCase = true) -> {
        val parts = s.substringAfter('(').substringBefore(')').split(',').map { it.trim() }
        val r = parts[0].toFloat() / 255f
        val g = parts[1].toFloat() / 255f
        val b = parts[2].toFloat() / 255f
        val a = if (parts.size >= 4) parts[3].toFloat().coerceIn(0f, 1f) else 1f
        Color(red = r.coerceIn(0f, 1f), green = g.coerceIn(0f, 1f), blue = b.coerceIn(0f, 1f), alpha = a)
      }
      else -> Color(android.graphics.Color.parseColor(s))
    }
  } catch (_: Exception) {
    null
  }
}

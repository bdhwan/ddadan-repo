package com.ddadan.player.ui

import androidx.compose.ui.graphics.Color

fun parseColorOrNull(value: String?): Color? {
  if (value.isNullOrBlank()) return null
  val normalized = value.trim()
  return try {
    when {
      normalized.startsWith("#") -> Color(android.graphics.Color.parseColor(normalized))
      else -> Color(android.graphics.Color.parseColor(normalized))
    }
  } catch (_: IllegalArgumentException) {
    null
  }
}

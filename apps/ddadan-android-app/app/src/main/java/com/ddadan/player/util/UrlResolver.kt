package com.ddadan.player.util

import com.ddadan.player.data.ScreenItem

fun absoluteUrl(item: ScreenItem, apiBase: String): String? {
  val url = item.url ?: return null
  if (url.startsWith("http")) return url
  return apiBase.removeSuffix("/api") + url
}

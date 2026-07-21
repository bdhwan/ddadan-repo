package com.ddadan.player.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import com.ddadan.player.data.ScreenItem

@Composable
fun ScreenStage(
  designWidth: Int,
  designHeight: Int,
  background: String?,
  items: List<ScreenItem>,
  apiBase: String,
  modifier: Modifier = Modifier,
) {
  BoxWithConstraints(
    modifier =
      modifier
        .fillMaxSize()
        .background(parseColorOrNull(background) ?: Color(0xFF0C0F1A)),
  ) {
    // Dp, to match the boxes below. Passing constraints.maxHeight here instead — which is
    // pixels — is what made text render density-times too large and clip out of its box.
    val stageHeightDp = maxHeight.value
    items.forEach { item ->
      val left = maxWidth * (item.x / designWidth).toFloat()
      val top = maxHeight * (item.y / designHeight).toFloat()
      val width = maxWidth * (item.width / designWidth).toFloat()
      val height = maxHeight * (item.height / designHeight).toFloat()

      Box(
        modifier =
          Modifier
            .offset(x = left, y = top)
            .size(width = width, height = height)
            .background(parseColorOrNull(item.background) ?: Color.Transparent),
      ) {
        ScreenItemContent(
          item = item,
          stageHeightDp = stageHeightDp,
          designHeight = designHeight,
          apiBase = apiBase,
        )
      }
    }
  }
}

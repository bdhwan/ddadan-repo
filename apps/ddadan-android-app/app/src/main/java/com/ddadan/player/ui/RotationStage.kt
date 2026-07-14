package com.ddadan.player.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import com.ddadan.player.data.SlidePayload

@Composable
fun RotationStage(
  slides: List<SlidePayload>,
  idx0: Int,
  idx1: Int,
  op0: Float,
  op1: Float,
  transition: Boolean,
  fadeMs: Long,
  apiBase: String,
  modifier: Modifier = Modifier,
) {
  val animatedOp0 by animateFloatAsState(
    targetValue = op0,
    animationSpec = if (transition) tween(durationMillis = fadeMs.toInt()) else tween(0),
    label = "rotOp0",
  )
  val animatedOp1 by animateFloatAsState(
    targetValue = op1,
    animationSpec = if (transition) tween(durationMillis = fadeMs.toInt()) else tween(0),
    label = "rotOp1",
  )

  val slide0 = slides.getOrNull(idx0)
  val slide1 = slides.getOrNull(idx1)

  Box(modifier = modifier.fillMaxSize()) {
    if (slide0 != null) {
      ScreenStage(
        designWidth = slide0.width,
        designHeight = slide0.height,
        background = slide0.background,
        items = slide0.items,
        apiBase = apiBase,
        modifier = Modifier.fillMaxSize().alpha(animatedOp0),
      )
    }
    if (slide1 != null) {
      ScreenStage(
        designWidth = slide1.width,
        designHeight = slide1.height,
        background = slide1.background,
        items = slide1.items,
        apiBase = apiBase,
        modifier = Modifier.fillMaxSize().alpha(animatedOp1),
      )
    }
  }
}

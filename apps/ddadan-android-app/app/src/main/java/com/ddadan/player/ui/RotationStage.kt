package com.ddadan.player.ui

import androidx.compose.animation.core.LinearEasing
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
  @Suppress("UNUSED_PARAMETER") op0: Float,
  op1: Float,
  transition: Boolean,
  fadeMs: Long,
  apiBase: String,
  modifier: Modifier = Modifier,
) {
  // 자연스러운 크로스페이드: 나가는 화면(slide0)은 불투명한 바닥으로 두고,
  // 들어오는 화면(slide1)만 위에서 0→1로 서서히 나타난다. 두 겹을 동시에
  // 페이드하면 중간에 배경이 비쳐 어두워지며 뚝뚝 끊겨 보이는 문제를 막는다.
  val animatedOp1 by animateFloatAsState(
    targetValue = op1,
    animationSpec = if (transition) tween(durationMillis = fadeMs.toInt(), easing = LinearEasing) else tween(0),
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
        modifier = Modifier.fillMaxSize(),
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

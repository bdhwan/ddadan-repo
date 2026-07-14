package com.ddadan.player.ui

import android.view.ViewGroup
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.PlayerView
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import coil3.request.crossfade
import com.ddadan.player.data.ScreenItem
import com.ddadan.player.util.absoluteUrl
import com.ddadan.player.util.resolveFontSizeDp

/**
 * Board text is sized in Dp (see [resolveFontSizeDp]) and converted here with the local
 * density, so the rendered pixel size is exactly what the layout reserved for it. Going
 * through `Density.toSp` also divides out the user's font-scale setting, which a signage
 * board must ignore — it has a fixed box to fit into.
 */
@Composable
private fun fontSizeOf(item: ScreenItem, stageHeightDp: Float, designHeight: Int): TextUnit {
  val dp = resolveFontSizeDp(item, stageHeightDp, designHeight)
  return with(LocalDensity.current) { dp.dp.toSp() }
}

/** The web player insets box text by 12px of design space; scale it like everything else. */
private fun textInset(stageHeightDp: Float, designHeight: Int): Dp =
  (12f * stageHeightDp / designHeight.coerceAtLeast(1)).dp

/**
 * Only the horizontal inset is applied. `Modifier.padding` is a hard constraint in Compose —
 * a vertical inset shrinks the box the line box must fit in, and the text is then clipped.
 * The web gets away with `padding: 12px` because its line box is free to overflow into the
 * padding (the item, not the text, is what has `overflow: hidden`), so a board authored to
 * fit its box on the web must not lose height to padding here.
 */

@Composable
fun ScreenItemContent(
  item: ScreenItem,
  stageHeightDp: Float,
  designHeight: Int,
  apiBase: String,
  modifier: Modifier = Modifier,
) {
  val isMenuLine = item.textVariant == "menuLine"
  val textAlign =
    when (item.textAlign ?: if (isMenuLine) "left" else "center") {
      "right" -> TextAlign.End
      "left" -> TextAlign.Start
      else -> TextAlign.Center
    }

  Box(modifier = modifier.fillMaxSize()) {
    when (item.kind) {
      "image" -> {
        val url = absoluteUrl(item, apiBase)
        if (url != null) {
          AsyncImage(
            model =
              ImageRequest.Builder(LocalContext.current)
                .data(url)
                .crossfade(true)
                .build(),
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier.fillMaxSize(),
          )
        }
      }
      "video" -> {
        val url = absoluteUrl(item, apiBase)
        if (url != null) {
          VideoPlayer(url = url, modifier = Modifier.fillMaxSize())
        }
      }
      else -> {
        if (isMenuLine) {
          MenuLineText(item = item, stageHeightDp = stageHeightDp, designHeight = designHeight)
        } else {
          val fontSize = fontSizeOf(item, stageHeightDp, designHeight)
          Box(
            modifier = Modifier.fillMaxSize().padding(horizontal = textInset(stageHeightDp, designHeight)),
            contentAlignment = Alignment.Center,
          ) {
            Text(
              text = item.text.orEmpty(),
              color = parseColorOrNull(item.color) ?: Color.White,
              fontSize = fontSize,
              fontWeight =
                item.fontWeight?.toInt()?.let { weight ->
                  when {
                    weight >= 700 -> FontWeight.Bold
                    weight >= 600 -> FontWeight.SemiBold
                    weight >= 500 -> FontWeight.Medium
                    else -> FontWeight.Normal
                  }
                } ?: FontWeight.Normal,
              textAlign = textAlign,
              // Unspecified = the font's natural line height. Forcing it to fontSize (a 1.0
              // ratio) leaves no room for ascenders or descenders, which sheared the bottom
              // off every Hangul jongseong.
              lineHeight = item.lineHeight?.let { fontSize * it.toFloat() } ?: TextUnit.Unspecified,
              modifier = Modifier.fillMaxWidth(),
            )
          }
        }
      }
    }
  }
}

@Composable
private fun MenuLineText(item: ScreenItem, stageHeightDp: Float, designHeight: Int) {
  val fontSize = fontSizeOf(item, stageHeightDp, designHeight)
  val color = parseColorOrNull(item.color) ?: Color.White
  Row(
    modifier = Modifier.fillMaxSize().padding(horizontal = textInset(stageHeightDp, designHeight)),
    verticalAlignment = Alignment.Bottom,
  ) {
    Text(
      text = item.text.orEmpty(),
      color = color,
      fontSize = fontSize,
      maxLines = 1,
      overflow = TextOverflow.Ellipsis,
    )
    Box(
      modifier =
        Modifier
          .weight(1f)
          .padding(horizontal = 12.dp)
          .padding(bottom = 4.dp),
    ) {
      Canvas(modifier = Modifier.fillMaxSize()) {
        val y = size.height * 0.85f
        var x = 0f
        while (x < size.width) {
          drawCircle(color = color.copy(alpha = 0.45f), radius = 1.5f, center = Offset(x, y))
          x += 8f
        }
      }
    }
    Text(
      text = item.textSecondary.orEmpty(),
      color = color,
      fontSize = fontSize,
      maxLines = 1,
    )
  }
}

@Composable
private fun VideoPlayer(url: String, modifier: Modifier = Modifier) {
  val context = LocalContext.current
  val player =
    remember(url) {
      ExoPlayer.Builder(context).build().apply {
        repeatMode = Player.REPEAT_MODE_ALL
        volume = 0f
        playWhenReady = true
        setMediaItem(MediaItem.fromUri(url))
        prepare()
      }
    }

  DisposableEffect(player) {
    onDispose { player.release() }
  }

  AndroidView(
    factory = { ctx ->
      PlayerView(ctx).apply {
        layoutParams =
          ViewGroup.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT,
          )
        useController = false
        this.player = player
      }
    },
    modifier = modifier,
  )
}

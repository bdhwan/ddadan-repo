package com.ddadan.player.ui

import android.view.ViewGroup
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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
import com.ddadan.player.util.resolveFontSizeSp

@Composable
fun ScreenItemContent(
  item: ScreenItem,
  stageHeightPx: Float,
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
          MenuLineText(item = item, stageHeightPx = stageHeightPx)
        } else {
          Text(
            text = item.text.orEmpty(),
            color = parseColorOrNull(item.color) ?: Color.White,
            fontSize = resolveFontSizeSp(item, stageHeightPx).sp,
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
            lineHeight =
              item.lineHeight?.let { (it * resolveFontSizeSp(item, stageHeightPx)).sp }
                ?: resolveFontSizeSp(item, stageHeightPx).sp,
            modifier = Modifier.fillMaxSize().padding(12.dp),
          )
        }
      }
    }
  }
}

@Composable
private fun MenuLineText(item: ScreenItem, stageHeightPx: Float) {
  val fontSize = resolveFontSizeSp(item, stageHeightPx).sp
  val color = parseColorOrNull(item.color) ?: Color.White
  Row(
    modifier = Modifier.fillMaxSize().padding(12.dp),
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

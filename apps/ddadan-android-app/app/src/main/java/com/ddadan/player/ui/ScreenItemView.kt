package com.ddadan.player.ui

import android.view.ViewGroup
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontStyle
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
import com.ddadan.player.data.Badge
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
        if (item.textVariant == "note") {
          NoteBox(item = item, stageHeightDp = stageHeightDp, designHeight = designHeight)
        } else if (item.textVariant == "groupHeader") {
          GroupHeaderText(item = item, stageHeightDp = stageHeightDp, designHeight = designHeight)
        } else if (isMenuLine) {
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

// 뱃지/강조 색 — client-app(app.scss)의 프리셋과 일치시킨다.
private val BadgeBestBg = Color(0xFF2F6FD0)
private val BadgeRecBg = Color(0xFF2F9E6E)
private val PriceExtraGreen = Color(0xFF2F9E6E)
private val TagInfoGray = Color(0xFF8A8F98)
private val TagWarnFg = Color(0xFF1F7A4D)
private val TagWarnBg = Color(0x242F9E6E)

/**
 * 메뉴 한 행: [원형 뱃지] 한글명  영문명 ····· 기본가 (+보조가) [ICED Only 태그].
 * 뱃지/영문/태그/이중가격은 모두 optional — 없으면 기존 단순 행과 동일하게 보인다.
 */
@Composable
private fun MenuLineText(item: ScreenItem, stageHeightDp: Float, designHeight: Int) {
  val fontSize = fontSizeOf(item, stageHeightDp, designHeight)
  val color = parseColorOrNull(item.color) ?: Color.White
  val priceColor = parseColorOrNull(item.priceColor) ?: color
  val badges = item.badges.orEmpty()
  val prefix = badges.filter { it.variant == "best" || it.variant == "rec" }
  val suffix = badges.filter { it.variant != "best" && it.variant != "rec" }

  Row(
    modifier = Modifier.fillMaxSize().padding(horizontal = textInset(stageHeightDp, designHeight)),
    verticalAlignment = Alignment.Bottom,
  ) {
    prefix.forEach { badge ->
      BadgePill(badge = badge, baseSize = fontSize)
      Spacer(modifier = Modifier.width(6.dp))
    }
    Text(
      text = item.text.orEmpty(),
      color = color,
      fontSize = fontSize,
      fontWeight = FontWeight.Medium,
      maxLines = 1,
      overflow = TextOverflow.Ellipsis,
    )
    item.textEn?.let { en ->
      Spacer(modifier = Modifier.width(5.dp))
      Text(
        text = en,
        color = color.copy(alpha = 0.5f),
        fontSize = fontSize * 0.46f,
        fontWeight = FontWeight.SemiBold,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
      )
    }
    suffix.forEach { badge ->
      Spacer(modifier = Modifier.width(6.dp))
      TagInline(badge = badge, baseSize = fontSize)
    }
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
      color = priceColor,
      fontSize = fontSize,
      fontWeight = FontWeight.Bold,
      maxLines = 1,
    )
    item.priceExtra?.let { extra ->
      Spacer(modifier = Modifier.width(3.dp))
      Text(
        text = extra,
        color = PriceExtraGreen,
        fontSize = fontSize * 0.62f,
        fontWeight = FontWeight.Bold,
        maxLines = 1,
      )
    }
  }
}

/** 라벨 앞 원형 뱃지(BEST=파란원, 추천=초록원). */
@Composable
private fun BadgePill(badge: Badge, baseSize: TextUnit) {
  val bg = if (badge.variant == "rec") BadgeRecBg else BadgeBestBg
  Box(
    modifier =
      Modifier
        .clip(RoundedCornerShape(percent = 50))
        .background(bg)
        .padding(horizontal = 6.dp, vertical = 2.dp),
    contentAlignment = Alignment.Center,
  ) {
    Text(
      text = badge.text,
      color = Color.White,
      fontSize = baseSize * 0.44f,
      fontWeight = FontWeight.Bold,
      maxLines = 1,
    )
  }
}

/** 라벨 뒤 인라인 태그(info=회색 이탤릭 "ICED Only", warn=초록 박스 "DECAF"). */
@Composable
private fun TagInline(badge: Badge, baseSize: TextUnit) {
  if (badge.variant == "warn") {
    Box(
      modifier =
        Modifier
          .clip(RoundedCornerShape(4.dp))
          .background(TagWarnBg)
          .padding(horizontal = 5.dp, vertical = 2.dp),
    ) {
      Text(
        text = badge.text,
        color = TagWarnFg,
        fontSize = baseSize * 0.5f,
        fontWeight = FontWeight.Bold,
        maxLines = 1,
      )
    }
  } else {
    Text(
      text = badge.text,
      color = TagInfoGray,
      fontSize = baseSize * 0.5f,
      fontStyle = FontStyle.Italic,
      fontWeight = FontWeight.SemiBold,
      maxLines = 1,
    )
  }
}

/** 안내 콜백 박스: 라운드 틴트 박스(배경은 ScreenStage가 적용) + 체크 + 문구. */
@Composable
private fun NoteBox(item: ScreenItem, stageHeightDp: Float, designHeight: Int) {
  val fontSize = fontSizeOf(item, stageHeightDp, designHeight)
  val color = parseColorOrNull(item.color) ?: Color.White
  Row(
    modifier = Modifier.fillMaxSize().padding(horizontal = 12.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
      text = "✓",
      color = color,
      fontSize = fontSize,
      fontWeight = FontWeight.Bold,
    )
    Spacer(modifier = Modifier.width(8.dp))
    Text(
      text = item.text.orEmpty(),
      color = color,
      fontSize = fontSize,
      fontWeight = FontWeight.SemiBold,
      maxLines = 1,
      overflow = TextOverflow.Ellipsis,
    )
  }
}

/** 카테고리 그룹 헤더: 한글 title + 영문 + 룰 라인 + 우측 라벨(EXTRA SIZE 등). */
@Composable
private fun GroupHeaderText(item: ScreenItem, stageHeightDp: Float, designHeight: Int) {
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
      fontWeight = FontWeight.Bold,
      maxLines = 1,
    )
    item.textEn?.let { en ->
      Spacer(modifier = Modifier.width(6.dp))
      Text(
        text = en,
        color = color.copy(alpha = 0.85f),
        fontSize = fontSize * 0.6f,
        fontWeight = FontWeight.Bold,
        maxLines = 1,
      )
    }
    Box(
      modifier =
        Modifier
          .weight(1f)
          .align(Alignment.CenterVertically)
          .padding(horizontal = 10.dp)
          .height(2.dp)
          .background(color.copy(alpha = 0.35f)),
    )
    item.textSecondary?.let { right ->
      Text(
        text = right,
        color = color.copy(alpha = 0.7f),
        fontSize = fontSize * 0.5f,
        fontWeight = FontWeight.Bold,
        maxLines = 1,
      )
    }
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

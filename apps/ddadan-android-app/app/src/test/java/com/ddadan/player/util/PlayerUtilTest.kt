package com.ddadan.player.util

import com.ddadan.player.data.ScreenItem
import com.ddadan.player.data.SlidePayload
import org.junit.Assert.assertEquals
import org.junit.Test

class FontSizeTest {
  private fun textItem(fontSize: Double, fontUnit: String? = null) =
    ScreenItem(
      id = "t",
      kind = "text",
      fontSize = fontSize,
      fontUnit = fontUnit,
      x = 0.0,
      y = 0.0,
      width = 100.0,
      height = 40.0,
    )

  @Test
  fun vh_isPercentOfStageHeight() {
    assertEquals(
      40f,
      resolveFontSizeDp(textItem(3.7037037037, "vh"), stageHeightDp = 1080f, designHeight = 1080),
      0.01f,
    )
  }

  @Test
  fun px_isInDesignSpace_soItScalesWithTheStage() {
    // A 40px font on a 1080-tall board: unchanged when the stage happens to be 1080 Dp…
    assertEquals(
      40f,
      resolveFontSizeDp(textItem(40.0), stageHeightDp = 1080f, designHeight = 1080),
      0.01f,
    )
    // …and scaled by the same factor as the item boxes when it is not. Returning a raw 40
    // here is what overflowed every text box on an 800 Dp tall tablet.
    assertEquals(
      29.63f,
      resolveFontSizeDp(textItem(40.0), stageHeightDp = 800f, designHeight = 1080),
      0.01f,
    )
  }

  @Test
  fun px_keepsTheAuthoredFontToBoxRatio() {
    // The real boards: a 60px caption inside an 80px box. It must stay at 0.75 of its box
    // on any stage, or it clips.
    val fontDp = resolveFontSizeDp(textItem(60.0), stageHeightDp = 800f, designHeight = 1080)
    val boxDp = 800f * (80f / 1080f)
    assertEquals(0.75f, fontDp / boxDp, 0.001f)
  }

  @Test
  fun missingFontSize_fallsBackToADefault() {
    val item = ScreenItem(id = "t", kind = "text", x = 0.0, y = 0.0, width = 10.0, height = 10.0)
    assertEquals(16f, resolveFontSizeDp(item, stageHeightDp = 800f, designHeight = 1080), 0.01f)
  }
}

class UrlResolverTest {
  @Test
  fun absoluteUrl_resolvesRelativeStaticPath() {
    val item =
      ScreenItem(
        id = "img1",
        kind = "image",
        url = "/static/assets/test.png",
        x = 0.0,
        y = 0.0,
        width = 100.0,
        height = 100.0,
      )
    assertEquals(
      "http://10.0.2.2:7800/static/assets/test.png",
      absoluteUrl(item, "http://10.0.2.2:7800/api"),
    )
  }

  @Test
  fun absoluteUrl_keepsAbsoluteHttpUrl() {
    val item =
      ScreenItem(
        id = "img2",
        kind = "image",
        url = "https://example.com/a.png",
        x = 0.0,
        y = 0.0,
        width = 100.0,
        height = 100.0,
      )
    assertEquals("https://example.com/a.png", absoluteUrl(item, "http://10.0.2.2:7800/api"))
  }
}

class RotationKeyTest {
  @Test
  fun buildRotationKey_changesWhenSlideContentChanges() {
    val slidesA =
      listOf(
        SlidePayload(
          width = 1920,
          height = 1080,
          items =
            listOf(
              ScreenItem(id = "a", kind = "text", x = 0.0, y = 0.0, width = 10.0, height = 10.0),
            ),
        ),
        SlidePayload(
          width = 1920,
          height = 1080,
          items =
            listOf(
              ScreenItem(id = "b", kind = "text", x = 0.0, y = 0.0, width = 10.0, height = 10.0),
            ),
        ),
      )
    val slidesB =
      slidesA.mapIndexed { index, slide ->
        if (index == 0) {
          slide.copy(
            items =
              listOf(
                ScreenItem(id = "c", kind = "text", x = 0.0, y = 0.0, width = 10.0, height = 10.0),
              ),
          )
        } else {
          slide
        }
      }

    val keyA = buildRotationKey(slidesA, 10000, 800)
    val keyB = buildRotationKey(slidesB, 10000, 800)
    assertEquals("2|10000|800|a||b", keyA)
    assertEquals("2|10000|800|c||b", keyB)
  }
}

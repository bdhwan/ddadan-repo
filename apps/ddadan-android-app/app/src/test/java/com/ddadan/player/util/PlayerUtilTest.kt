package com.ddadan.player.util

import com.ddadan.player.data.ScreenItem
import com.ddadan.player.data.SlidePayload
import org.junit.Assert.assertEquals
import org.junit.Test

class FontSizeTest {
  @Test
  fun resolveFontSizeSp_returnsVhBasedSize() {
    val item =
      ScreenItem(
        id = "t1",
        kind = "text",
        fontSize = 3.7037037037,
        fontUnit = "vh",
        x = 0.0,
        y = 0.0,
        width = 100.0,
        height = 40.0,
      )
    assertEquals(40f, resolveFontSizeSp(item, stageHeightPx = 1080f), 0.01f)
  }

  @Test
  fun resolveFontSizeSp_returnsPxForLegacyData() {
    val item =
      ScreenItem(
        id = "t2",
        kind = "text",
        fontSize = 40.0,
        x = 0.0,
        y = 0.0,
        width = 100.0,
        height = 40.0,
      )
    assertEquals(40f, resolveFontSizeSp(item, stageHeightPx = 1080f))
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

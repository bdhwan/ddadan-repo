package com.ddadan.player.util

import com.ddadan.player.data.ScreenItem
import com.ddadan.player.data.SlidePayload
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
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
  private fun item(id: String, text: String? = null, price: String? = null) =
    ScreenItem(
      id = id, kind = "text", text = text, textSecondary = price,
      x = 0.0, y = 0.0, width = 10.0, height = 10.0,
    )

  private fun slides(vararg its: ScreenItem) =
    its.map { SlidePayload(width = 1920, height = 1080, items = listOf(it)) }

  @Test
  fun changesWhenItemIdChanges() {
    val a = slides(item("a"), item("b"))
    val b = slides(item("c"), item("b"))
    assertNotEquals(buildRotationKey(a, 10000, 800), buildRotationKey(b, 10000, 800))
  }

  /**
   * 실제로 겪은 버그: 메뉴 가격만 고치면 id 는 그대로라 키가 같았고, 플레이어가 옛 화면을
   * 계속 렌더해 재시작해야만 반영됐다. 이 케이스가 가장 흔한 편집이다.
   */
  @Test
  fun changesWhenOnlyPriceChanges() {
    val before = slides(item("row1", "딸기쉐이크", "7,000"))
    val after = slides(item("row1", "딸기쉐이크", "6,000"))
    assertNotEquals(
      buildRotationKey(before, 10000, 800),
      buildRotationKey(after, 10000, 800),
    )
  }

  @Test
  fun changesWhenOnlyTextChanges() {
    val before = slides(item("row1", "생과일주스"))
    val after = slides(item("row1", "생과일 주스"))
    assertNotEquals(
      buildRotationKey(before, 10000, 800),
      buildRotationKey(after, 10000, 800),
    )
  }

  /** 보조가(EXTRA SIZE의 "+1.0") 제거처럼 필드 하나가 비워지는 편집도 감지돼야 한다. */
  @Test
  fun changesWhenPriceExtraCleared() {
    val before = slides(item("row1", "아메리카노", "3.0").copy(priceExtra = "+1.0"))
    val after = slides(item("row1", "아메리카노", "3.0").copy(priceExtra = ""))
    assertNotEquals(
      buildRotationKey(before, 10000, 800),
      buildRotationKey(after, 10000, 800),
    )
  }

  @Test
  fun changesWhenIntervalOrFadeChanges() {
    val s = slides(item("a"), item("b"))
    assertNotEquals(buildRotationKey(s, 10000, 800), buildRotationKey(s, 18000, 800))
    assertNotEquals(buildRotationKey(s, 10000, 800), buildRotationKey(s, 10000, 900))
  }

  /** 내용이 같으면 키도 같아야 한다 — 매 폴링(5초)마다 로테이션이 리셋되면 안 된다. */
  @Test
  fun stableWhenNothingChanged() {
    val a = slides(item("row1", "아메리카노", "3.0"), item("row2", "카페라떼", "4.0"))
    val b = slides(item("row1", "아메리카노", "3.0"), item("row2", "카페라떼", "4.0"))
    assertEquals(buildRotationKey(a, 10000, 800), buildRotationKey(b, 10000, 800))
  }
}

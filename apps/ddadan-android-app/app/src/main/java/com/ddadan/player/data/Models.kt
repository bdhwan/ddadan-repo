package com.ddadan.player.data

import kotlinx.serialization.Serializable

@Serializable
data class Badge(
  val text: String,
  /** best=파란원, rec=초록원, info=회색 이탤릭, warn=초록박스. */
  val variant: String? = null,
)

@Serializable
data class ScreenItem(
  val id: String,
  val kind: String,
  val url: String? = null,
  val text: String? = null,
  val fontSize: Double? = null,
  val fontUnit: String? = null,
  val color: String? = null,
  val background: String? = null,
  val opacity: Double? = null,
  val fontWeight: Double? = null,
  val textAlign: String? = null,
  val lineHeight: Double? = null,
  /** plain | menuLine | groupHeader */
  val textVariant: String? = null,
  val textSecondary: String? = null,
  /** 영문 병기(한글명 옆 작은 회색 텍스트). */
  val textEn: String? = null,
  /** 이중 가격의 보조가("+1,000" 등) — 기본가 뒤 초록 강조. */
  val priceExtra: String? = null,
  /** menuLine 가격 색(미지정 시 아이템 색 상속). */
  val priceColor: String? = null,
  /** 인라인 뱃지(BEST/추천/ICED Only/DECAF 등). */
  val badges: List<Badge>? = null,
  val x: Double,
  val y: Double,
  val width: Double,
  val height: Double,
  val zIndex: Int? = null,
  /** 배경 모서리 둥글기(px, 디자인 좌표계). 그룹 카드/패널/안내박스. */
  val radius: Double? = null,
)

@Serializable
data class SlidePayload(
  val width: Int,
  val height: Int,
  val background: String? = null,
  val items: List<ScreenItem> = emptyList(),
)

@Serializable
data class RotationConfig(
  val intervalMs: Long,
  val fadeMs: Long,
  val slides: List<SlidePayload> = emptyList(),
)

@Serializable
data class ScreenResponse(
  val registered: Boolean,
  val deviceName: String? = null,
  val mode: String? = null,
  val width: Int,
  val height: Int,
  val background: String? = null,
  val items: List<ScreenItem> = emptyList(),
  val rotation: RotationConfig? = null,
  val isFallback: Boolean? = null,
)

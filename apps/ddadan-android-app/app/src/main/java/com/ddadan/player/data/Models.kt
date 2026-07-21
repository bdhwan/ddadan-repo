package com.ddadan.player.data

import kotlinx.serialization.Serializable

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
  val textVariant: String? = null,
  val textSecondary: String? = null,
  val x: Double,
  val y: Double,
  val width: Double,
  val height: Double,
  val zIndex: Int? = null,
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
data class UploadAck(
  val ok: Boolean = false,
  val id: Int = 0,
)

@Serializable
data class ApkInfo(
  val versionCode: Int = 0,
  val versionName: String? = null,
  val applicationId: String? = null,
  val url: String? = null,
  val sizeBytes: Long = 0,
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

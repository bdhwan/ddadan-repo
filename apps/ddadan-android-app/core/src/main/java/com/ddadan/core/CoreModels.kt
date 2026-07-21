package com.ddadan.core

import kotlinx.serialization.Serializable

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

/** 디바이스 자원 텔레메트리(heartbeat 겸용). */
@Serializable
data class TelemetryBody(
  val appVersion: String? = null,
  val cpuPercent: Double? = null,
  val ramUsedMb: Long? = null,
  val ramTotalMb: Long? = null,
  val diskUsedPercent: Double? = null,
)

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

/** 서버에서 받은 원격 명령. */
@Serializable
data class DeviceCommandDto(
  val id: Int = 0,
  val type: String = "",
  val payload: String? = null,
  val status: String = "pending",
)

@Serializable
data class AckBody(
  val status: String,
  val result: String? = null,
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

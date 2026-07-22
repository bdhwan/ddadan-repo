package com.ddadan.player

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** OTA 진행 상태(브로드캐스트로 갱신). null이면 진행 중 아님. */
data class OtaStatus(val app: String, val phase: String, val percent: Int)

object OtaProgress {
  private val _status = MutableStateFlow<OtaStatus?>(null)
  val status: StateFlow<OtaStatus?> = _status.asStateFlow()

  fun update(status: OtaStatus?) {
    _status.value = status
  }
}

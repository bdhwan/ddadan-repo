package com.ddadan.core

/**
 * OTA 진행률을 워치독(수행) → 플레이어(표시)로 전달하는 브로드캐스트 계약.
 * 두 앱이 별도 프로세스/패키지라 파일 공유 대신 브로드캐스트를 쓴다.
 */
object OtaBroadcast {
  const val ACTION = "com.ddadan.OTA_PROGRESS"
  const val EXTRA_APP = "app" // 갱신 대상 applicationId
  const val EXTRA_PHASE = "phase" // "downloading" | "installing" | "done"
  const val EXTRA_PERCENT = "percent" // 0..100 (없으면 -1)

  const val PHASE_DOWNLOADING = "downloading"
  const val PHASE_INSTALLING = "installing"
  const val PHASE_DONE = "done"

  /** 진행 오버레이를 그리는 플레이어 패키지(브로드캐스트 타깃). */
  const val PLAYER_PACKAGE = "com.ddadan.player"
}

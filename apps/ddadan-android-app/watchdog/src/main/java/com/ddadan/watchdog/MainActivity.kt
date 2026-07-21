package com.ddadan.watchdog

import android.app.Activity
import android.os.Bundle

/** UI 없는 진입점: 서비스만 띄우고 즉시 종료(크라이저 Monitor와 동일 패턴). */
class MainActivity : Activity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    WatchdogService.start(this)
    finish()
  }
}

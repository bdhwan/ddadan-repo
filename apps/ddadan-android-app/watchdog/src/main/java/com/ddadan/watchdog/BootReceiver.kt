package com.ddadan.watchdog

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** 부팅 완료 시 워치독 서비스 시작. */
class BootReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action == Intent.ACTION_BOOT_COMPLETED) {
      WatchdogService.start(context)
    }
  }
}

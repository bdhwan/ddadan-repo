package com.ddadan.player

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Relaunches the player after the device boots (or the app is updated / re-enabled),
 * so a signage tablet comes back on screen without anyone touching it.
 *
 * This covers the "auto-start" half. The "stays on screen" half is the CATEGORY_HOME
 * intent filter on MainActivity: once the user picks this app as the launcher, HOME
 * returns here too, so on most boots Android brings MainActivity up as the home screen
 * on its own and this receiver is just a belt-and-braces path (and the one that matters
 * on OEMs that don't auto-launch the home app after a locked boot).
 */
class BootReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent?) {
    when (intent?.action) {
      Intent.ACTION_BOOT_COMPLETED,
      Intent.ACTION_LOCKED_BOOT_COMPLETED,
      Intent.ACTION_MY_PACKAGE_REPLACED,
      "android.intent.action.QUICKBOOT_POWERON", // HTC/Xiaomi/Samsung fast-boot variant
      -> launchPlayer(context)
    }
  }

  private fun launchPlayer(context: Context) {
    val launch =
      Intent(context, MainActivity::class.java).apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      }
    context.startActivity(launch)
  }
}

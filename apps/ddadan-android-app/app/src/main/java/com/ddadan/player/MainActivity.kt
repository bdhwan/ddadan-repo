package com.ddadan.player

import android.app.ActivityManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import com.ddadan.core.OtaBroadcast
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.compose.runtime.LaunchedEffect
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.ddadan.player.data.PlayerRepository
import com.ddadan.player.prefs.PlayerPreferences
import com.ddadan.player.ui.PlayerScreen
import com.ddadan.player.ui.theme.DDADANPlayerTheme
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
  private val otaAutoClear = Handler(Looper.getMainLooper())
  private val otaClearRunnable = Runnable { OtaProgress.update(null) }

  /** 워치독의 OTA 진행률 브로드캐스트 수신 → 좌상단 오버레이 상태 갱신. */
  private val otaReceiver =
    object : BroadcastReceiver() {
      override fun onReceive(context: Context, intent: Intent) {
        val phase = intent.getStringExtra(OtaBroadcast.EXTRA_PHASE) ?: return
        otaAutoClear.removeCallbacks(otaClearRunnable)
        if (phase == OtaBroadcast.PHASE_DONE) {
          OtaProgress.update(null)
        } else {
          OtaProgress.update(
            OtaStatus(
              app = intent.getStringExtra(OtaBroadcast.EXTRA_APP).orEmpty(),
              phase = phase,
              percent = intent.getIntExtra(OtaBroadcast.EXTRA_PERCENT, -1),
            ),
          )
          // 안전장치: 15초간 갱신 없으면 자동 숨김(설치 중 프로세스 종료 등).
          otaAutoClear.postDelayed(otaClearRunnable, 15_000)
        }
      }
    }

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)

    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    registerReceiver(otaReceiver, IntentFilter(OtaBroadcast.ACTION))
    startWatchdogKeeper()
    enableEdgeToEdge()
    WindowCompat.setDecorFitsSystemWindows(window, false)
    WindowInsetsControllerCompat(window, window.decorView).apply {
      hide(WindowInsetsCompat.Type.systemBars())
      systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
    }

    val preferences = PlayerPreferences(applicationContext)
    val repository = PlayerRepository(preferences)
    val factory = PlayerViewModelFactory(repository, preferences)

    val deviceId = intent.getStringExtra(EXTRA_DEVICE_ID)
    val slot = intent.getIntExtra(EXTRA_SLOT, -1).takeIf { it >= 0 }
    val apiBase = intent.getStringExtra(EXTRA_API_BASE)

    setContent {
      DDADANPlayerTheme {
        Surface(modifier = Modifier.fillMaxSize(), color = Color(0xFF050505)) {
          val viewModel: PlayerViewModel = viewModel(factory = factory)
          LaunchedEffect(deviceId, slot, apiBase) {
            viewModel.seedFromIntent(deviceId, slot, apiBase)
            viewModel.start()
          }
          PlayerScreen(viewModel = viewModel)
        }
      }
    }
  }

  override fun onDestroy() {
    otaAutoClear.removeCallbacks(otaClearRunnable)
    try {
      unregisterReceiver(otaReceiver)
    } catch (_: Exception) {
    }
    super.onDestroy()
  }

  /**
   * 상호 감시: 워치독이 죽으면(예: 워치독 자가-OTA 설치로 종료) 다시 띄운다.
   * 워치독이 플레이어를 감시하는 것과 대칭 — 이걸로 워치독 자가-OTA가 견고해진다(재부팅 불필요).
   */
  private fun startWatchdogKeeper() {
    lifecycleScope.launch {
      while (isActive) {
        try {
          if (!isWatchdogRunning()) {
            packageManager.getLaunchIntentForPackage(WATCHDOG_PACKAGE)?.let {
              it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
              startActivity(it)
            }
          }
        } catch (e: Exception) {
          Log.w("MainActivity", "watchdog keeper error: ${e.message}")
        }
        delay(WATCHDOG_CHECK_MS)
      }
    }
  }

  private fun isWatchdogRunning(): Boolean {
    val am = getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
    return am.runningAppProcesses?.any { it.processName == WATCHDOG_PACKAGE } == true
  }

  companion object {
    const val EXTRA_DEVICE_ID = "deviceId"
    const val EXTRA_SLOT = "slot"
    const val EXTRA_API_BASE = "apiBase"
    private const val WATCHDOG_PACKAGE = "com.ddadan.watchdog"
    private const val WATCHDOG_CHECK_MS = 5_000L
  }
}

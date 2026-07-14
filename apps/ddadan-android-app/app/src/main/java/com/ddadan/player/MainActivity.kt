package com.ddadan.player

import android.content.Intent
import android.os.Bundle
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
import androidx.lifecycle.viewmodel.compose.viewModel
import com.ddadan.player.data.PlayerRepository
import com.ddadan.player.prefs.PlayerPreferences
import com.ddadan.player.ui.PlayerScreen
import com.ddadan.player.ui.theme.DDADANPlayerTheme

class MainActivity : ComponentActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)

    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
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

  companion object {
    const val EXTRA_DEVICE_ID = "deviceId"
    const val EXTRA_SLOT = "slot"
    const val EXTRA_API_BASE = "apiBase"
  }
}

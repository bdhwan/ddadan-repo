package com.ddadan.player.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.ddadan.player.BuildConfig
import com.ddadan.player.PlayerUiState
import com.ddadan.player.PlayerViewModel
import com.ddadan.player.PlayerViewModelFactory
import com.ddadan.player.data.PlayerRepository
import com.ddadan.player.prefs.PlayerPreferences

@Composable
fun PlayerScreen(
  viewModel: PlayerViewModel =
    viewModel(
      factory =
        PlayerViewModelFactory(
          repository = PlayerRepository(PlayerPreferences(LocalContext.current.applicationContext)),
          preferences = PlayerPreferences(LocalContext.current.applicationContext),
        ),
    ),
) {
  val state by viewModel.uiState.collectAsStateWithLifecycle()
  val needsRegister = viewModel.needsRegister

  Box(
    modifier =
      Modifier
        .fillMaxSize()
        .background(Color(0xFF050505))
        .onKeyEvent { event ->
          if (event.type == KeyEventType.KeyDown && event.key == Key.Menu) {
            viewModel.openSettingsEditor()
            true
          } else {
            false
          }
        }
        .focusable(),
  ) {
    when {
      state.useRotation -> {
        val slides = state.rotationSlides!!
        val fadeMs = state.screen?.rotation?.fadeMs ?: 800L
        RotationStage(
          slides = slides,
          idx0 = state.rotIdx0,
          idx1 = state.rotIdx1,
          op0 = state.rotOp0,
          op1 = state.rotOp1,
          transition = state.rotTransition,
          fadeMs = fadeMs,
          apiBase = state.apiBase,
          modifier = Modifier.fillMaxSize(),
        )
      }
      state.screen != null -> {
        val screen = state.screen!!
        ScreenStage(
          designWidth = screen.width,
          designHeight = screen.height,
          background = screen.background,
          items = screen.items,
          apiBase = state.apiBase,
          modifier = Modifier.fillMaxSize(),
        )
      }
      state.isLoading -> {
        Text(
          text = "DDADAN 플레이어 연결 중...",
          color = Color.White.copy(alpha = 0.7f),
          fontSize = 24.sp,
          modifier = Modifier.align(Alignment.Center),
        )
      }
    }

    DeviceIdOverlay(
      visible = needsRegister,
      hardwareId = state.hardwareId,
      editing = state.editingDeviceId,
      draftDeviceId = state.draftDeviceId,
      onOpenEditor = viewModel::openDeviceIdEditor,
      onDraftChange = viewModel::updateDraftDeviceId,
      onApply = viewModel::applyDeviceId,
      onCancel = viewModel::cancelDeviceIdEditor,
    )

    if (state.editingSettings) {
      Box(
        modifier =
          Modifier
            .fillMaxSize()
            .background(Color(0xB305070E))
            .clickable(onClick = viewModel::cancelSettingsEditor),
        contentAlignment = Alignment.Center,
      ) {
        Box(modifier = Modifier.clickable(enabled = false, onClick = {})) {
          SettingsDialog(
            draftApiBase = state.draftApiBase,
            draftSlot = state.draftSlot,
            defaultApiBase = BuildConfig.API_BASE,
            onApiBaseChange = viewModel::updateDraftApiBase,
            onSlotChange = viewModel::updateDraftSlot,
            onApply = { viewModel.applySettings(state.draftApiBase, state.draftSlot) },
            onResetApiBase = viewModel::resetApiBase,
            onCancel = viewModel::cancelSettingsEditor,
          )
        }
      }
    }

    SettingsHintBadge(
      onClick = viewModel::openSettingsEditor,
      modifier = Modifier.align(Alignment.TopStart).padding(16.dp),
    )
  }
}

@Composable
private fun SettingsHintBadge(onClick: () -> Unit, modifier: Modifier = Modifier) {
  Text(
    text = "설정 (Menu)",
    color = Color.White.copy(alpha = 0.35f),
    fontSize = 12.sp,
    modifier =
      modifier
        .clickable(onClick = onClick)
        .focusable()
        .background(Color(0x66141824))
        .padding(horizontal = 10.dp, vertical = 6.dp),
  )
}

private val PlayerUiState.useRotation: Boolean
  get() = (rotationSlides?.size ?: 0) >= 2

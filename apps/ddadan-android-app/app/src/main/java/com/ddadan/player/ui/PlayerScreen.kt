package com.ddadan.player.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
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

    if (state.discovering) {
      DiscoveryOverlay(
        scanCurrentIp = state.scanCurrentIp,
        scanDone = state.scanDone,
        scanTotal = state.scanTotal,
        retryCountdownSec = state.retryCountdownSec,
        modifier = Modifier.fillMaxSize(),
      )
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

  }
}

@Composable
private fun DiscoveryOverlay(
  scanCurrentIp: String?,
  scanDone: Int,
  scanTotal: Int,
  retryCountdownSec: Int,
  modifier: Modifier = Modifier,
) {
  Box(
    modifier = modifier.background(Color(0xF2050505)),
    contentAlignment = Alignment.Center,
  ) {
    Column(
      horizontalAlignment = Alignment.CenterHorizontally,
      verticalArrangement = Arrangement.Center,
    ) {
      Text(
        text = "서버를 찾는 중",
        color = Color.White.copy(alpha = 0.9f),
        fontSize = 28.sp,
      )
      Spacer(modifier = Modifier.height(20.dp))
      if (retryCountdownSec > 0) {
        Text(
          text = "이 네트워크에서 서버를 찾지 못했습니다",
          color = Color.White.copy(alpha = 0.6f),
          fontSize = 18.sp,
          textAlign = TextAlign.Center,
        )
        Spacer(modifier = Modifier.height(8.dp))
        Text(
          text = "${retryCountdownSec}초 후 다시 탐색합니다",
          color = Color(0xFFFFC107),
          fontSize = 22.sp,
        )
      } else {
        Text(
          text = scanCurrentIp ?: "네트워크 스캔 준비 중...",
          color = Color.White.copy(alpha = 0.85f),
          fontSize = 24.sp,
        )
        if (scanTotal > 0) {
          Spacer(modifier = Modifier.height(8.dp))
          Text(
            text = "$scanDone / $scanTotal",
            color = Color.White.copy(alpha = 0.5f),
            fontSize = 16.sp,
          )
        }
      }
    }
  }
}

private val PlayerUiState.useRotation: Boolean
  get() = (rotationSlides?.size ?: 0) >= 2

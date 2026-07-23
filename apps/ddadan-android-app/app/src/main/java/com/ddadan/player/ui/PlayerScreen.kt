package com.ddadan.player.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import com.ddadan.player.util.gatherNetDiag
import kotlinx.coroutines.delay
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
import com.ddadan.core.OtaBroadcast
import com.ddadan.player.BuildConfig
import com.ddadan.player.OtaProgress
import com.ddadan.player.OtaStatus
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
      state.awaitingNetwork -> {
        Text(
          text = "네트워크 연결 대기 중...",
          color = Color.White.copy(alpha = 0.7f),
          fontSize = 24.sp,
          modifier = Modifier.align(Alignment.Center),
        )
      }
      state.isLoading -> {
        Text(
          text = "브로트베르크 연결 중...",
          color = Color.White.copy(alpha = 0.7f),
          fontSize = 24.sp,
          modifier = Modifier.align(Alignment.Center),
        )
      }
    }

    when {
      // 보여줄 콘텐츠가 없이 탐색 중이면(첫 부팅 등) 기존 전체 탐색 화면.
      state.discovering && state.screen == null -> {
        DiscoveryOverlay(
          scanCurrentIp = state.scanCurrentIp,
          scanDone = state.scanDone,
          scanTotal = state.scanTotal,
          retryCountdownSec = state.retryCountdownSec,
          hardwareId = state.hardwareId,
          apiBase = state.apiBase,
          modifier = Modifier.fillMaxSize(),
        )
      }
      // 콘텐츠가 있는데 재연결 유예 중이거나 탐색 중이면, 화면은 그대로 두고
      // 좌상단에 작게 재연결 상태만 표시한다.
      (state.reconnecting || state.discovering) && state.screen != null -> {
        ReconnectingBadge(
          retryCountdownSec = state.retryCountdownSec,
          modifier = Modifier.align(Alignment.TopStart).padding(12.dp),
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

    // OTA 진행 중 좌상단 작은 오버레이.
    val ota by OtaProgress.status.collectAsStateWithLifecycle()
    ota?.let {
      OtaOverlay(
        status = it,
        modifier = Modifier.align(Alignment.TopStart).padding(12.dp),
      )
    }
  }
}

/**
 * 콘텐츠가 이미 떠 있는 상태에서 서버 연결이 잠깐 끊겼을 때, 화면을 덮지 않고
 * 좌상단에 작게 재연결 상태만 보여준다. 마지막 메뉴는 그대로 유지된다.
 */
@Composable
private fun ReconnectingBadge(retryCountdownSec: Int, modifier: Modifier = Modifier) {
  val label =
    if (retryCountdownSec > 0) "서버 재연결 중 · ${retryCountdownSec}초 후 재시도"
    else "서버 재연결 중…"
  Row(
    modifier =
      modifier
        .clip(RoundedCornerShape(8.dp))
        .background(Color(0xE6141A24))
        .padding(horizontal = 12.dp, vertical = 8.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
      text = label,
      color = Color(0xFFFFC107),
      fontSize = 14.sp,
      fontWeight = FontWeight.SemiBold,
    )
  }
}

/** OTA 진행 중 좌상단에 작게 표시하는 오버레이. */
@Composable
private fun OtaOverlay(status: OtaStatus, modifier: Modifier = Modifier) {
  val label =
    when (status.phase) {
      OtaBroadcast.PHASE_INSTALLING -> "APK 설치 중…"
      else -> if (status.percent >= 0) "APK 업데이트 중  ${status.percent}%" else "APK 업데이트 중…"
    }
  Row(
    modifier =
      modifier
        .clip(RoundedCornerShape(8.dp))
        .background(Color(0xE6141A24))
        .padding(horizontal = 12.dp, vertical = 8.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(
      text = label,
      color = Color(0xFFFFC107),
      fontSize = 14.sp,
      fontWeight = FontWeight.SemiBold,
    )
  }
}

@Composable
private fun DiscoveryOverlay(
  scanCurrentIp: String?,
  scanDone: Int,
  scanTotal: Int,
  retryCountdownSec: Int,
  hardwareId: String,
  apiBase: String,
  modifier: Modifier = Modifier,
) {
  val context = LocalContext.current
  // 네트워크 상태는 바뀔 수 있으니 3초마다 갱신.
  val diag by produceState(initialValue = gatherNetDiag(context), context) {
    while (true) {
      value = gatherNetDiag(context)
      delay(3000)
    }
  }

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
      Spacer(modifier = Modifier.height(16.dp))
      if (retryCountdownSec > 0) {
        Text(
          text = "이 네트워크에서 서버를 찾지 못했습니다",
          color = Color.White.copy(alpha = 0.6f),
          fontSize = 18.sp,
          textAlign = TextAlign.Center,
        )
        Spacer(modifier = Modifier.height(6.dp))
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
          Spacer(modifier = Modifier.height(6.dp))
          Text(
            text = "$scanDone / $scanTotal",
            color = Color.White.copy(alpha = 0.5f),
            fontSize = 16.sp,
          )
        }
      }

      Spacer(modifier = Modifier.height(24.dp))

      // ── 진단 정보 패널 ── 서버에 못 붙는 원인 파악용.
      val noNetwork = diag.ips.isEmpty()
      Column(
        modifier =
          Modifier
            .clip(RoundedCornerShape(12.dp))
            .background(Color(0x14FFFFFF))
            .padding(horizontal = 24.dp, vertical = 18.dp),
        horizontalAlignment = Alignment.Start,
      ) {
        Text(
          text = "기기 진단 정보",
          color = Color.White.copy(alpha = 0.5f),
          fontSize = 14.sp,
          fontWeight = FontWeight.Bold,
        )
        Spacer(modifier = Modifier.height(10.dp))
        DiagRow("기기 ID", hardwareId)
        DiagRow(
          label = "IP 주소",
          value = if (noNetwork) "없음 — 네트워크 미연결" else diag.ips.joinToString("   "),
          valueColor = if (noNetwork) Color(0xFFFF5252) else Color.White,
        )
        DiagRow(
          label = "WiFi",
          value = listOfNotNull(diag.ssid, diag.wifiState).joinToString(" · "),
          valueColor = if (diag.wifiState == "연결됨") Color.White else Color(0xFFFFC107),
        )
        DiagRow("게이트웨이", diag.gateway ?: "없음")
        DiagRow("스캔 대상", diag.subnet?.let { "$it (2~254)" } ?: "—")
        DiagRow("서버 주소(대상)", apiBase)
        DiagRow("앱 버전", "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})")
      }
    }
  }
}

@Composable
private fun DiagRow(label: String, value: String, valueColor: Color = Color.White) {
  Row(modifier = Modifier.padding(vertical = 3.dp)) {
    Text(
      text = label,
      color = Color.White.copy(alpha = 0.45f),
      fontSize = 16.sp,
      modifier = Modifier.width(150.dp),
    )
    Text(
      text = value,
      color = valueColor.copy(alpha = if (valueColor == Color.White) 0.9f else 1f),
      fontSize = 16.sp,
      fontFamily = FontFamily.Monospace,
    )
  }
}

private val PlayerUiState.useRotation: Boolean
  get() = (rotationSlides?.size ?: 0) >= 2

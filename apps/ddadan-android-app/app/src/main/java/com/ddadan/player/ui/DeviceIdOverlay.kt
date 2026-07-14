package com.ddadan.player.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

@Composable
fun DeviceIdBadge(
  hardwareId: String,
  onClick: () -> Unit,
  modifier: Modifier = Modifier,
) {
  Row(
    modifier =
      modifier
        .clickable(onClick = onClick)
        .focusable()
        .background(Color(0xD9141824), RoundedCornerShape(999.dp))
        .padding(horizontal = 16.dp, vertical = 10.dp),
    horizontalArrangement = Arrangement.spacedBy(10.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(text = "등록코드:", color = Color.White, fontSize = 14.sp)
    Text(text = hardwareId, color = Color.White, fontSize = 14.sp)
    Text(
      text = "변경",
      color = Color.White,
      fontSize = 12.sp,
      modifier =
        Modifier
          .background(Color(0xFF2C7BE5), RoundedCornerShape(999.dp))
          .padding(horizontal = 8.dp, vertical = 2.dp),
    )
  }
}

@Composable
fun DeviceIdDialog(
  draftDeviceId: String,
  onDraftChange: (String) -> Unit,
  onApply: (String) -> Unit,
  onCancel: () -> Unit,
) {
  val focusRequester = remember { FocusRequester() }
  Dialog(
    onDismissRequest = onCancel,
    properties = DialogProperties(usePlatformDefaultWidth = false),
  ) {
    Column(
      modifier =
        Modifier
          .fillMaxWidth(0.92f)
          .background(Color(0xFF11151F), RoundedCornerShape(16.dp))
          .padding(24.dp),
      verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
      Text(text = "등록코드 입력", color = Color.White, fontSize = 20.sp)
      Text(
        text = "디바이스에 부여된 등록코드를 입력하세요.",
        color = Color.White.copy(alpha = 0.65f),
        fontSize = 13.sp,
      )
      OutlinedTextField(
        value = draftDeviceId,
        onValueChange = onDraftChange,
        singleLine = true,
        modifier = Modifier.fillMaxWidth().focusRequester(focusRequester).focusable(),
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = { onApply(draftDeviceId) }),
      )
      Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.align(Alignment.End)) {
        OutlinedButton(onClick = onCancel) { Text("취소") }
        Button(onClick = { onApply(draftDeviceId) }) { Text("적용") }
      }
    }
  }
  LaunchedEffect(Unit) { focusRequester.requestFocus() }
}

@Composable
fun SettingsDialog(
  draftApiBase: String,
  draftSlot: String,
  defaultApiBase: String,
  onApiBaseChange: (String) -> Unit,
  onSlotChange: (String) -> Unit,
  onApply: () -> Unit,
  onResetApiBase: () -> Unit,
  onCancel: () -> Unit,
) {
  val focusRequester = remember { FocusRequester() }
  Dialog(
    onDismissRequest = onCancel,
    properties = DialogProperties(usePlatformDefaultWidth = false),
  ) {
    Column(
      modifier =
        Modifier
          .fillMaxWidth(0.92f)
          .background(Color(0xFF11151F), RoundedCornerShape(16.dp))
          .padding(24.dp),
      verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
      Text(text = "플레이어 설정", color = Color.White, fontSize = 20.sp)
      Text(
        text = "API 서버 주소와 모니터 슬롯을 설정합니다. 기본 API: $defaultApiBase",
        color = Color.White.copy(alpha = 0.65f),
        fontSize = 13.sp,
      )
      OutlinedTextField(
        value = draftApiBase,
        onValueChange = onApiBaseChange,
        label = { Text("API 서버") },
        singleLine = true,
        modifier = Modifier.fillMaxWidth().focusRequester(focusRequester).focusable(),
      )
      OutlinedTextField(
        value = draftSlot,
        onValueChange = onSlotChange,
        label = { Text("슬롯") },
        singleLine = true,
        modifier = Modifier.fillMaxWidth().focusable(),
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = { onApply() }),
      )
      Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.align(Alignment.End)) {
        OutlinedButton(onClick = onResetApiBase) { Text("API 초기화") }
        OutlinedButton(onClick = onCancel) { Text("취소") }
        Button(onClick = onApply) { Text("적용") }
      }
    }
  }
  LaunchedEffect(Unit) { focusRequester.requestFocus() }
}

@Composable
fun DeviceIdOverlay(
  visible: Boolean,
  hardwareId: String,
  editing: Boolean,
  draftDeviceId: String,
  onOpenEditor: () -> Unit,
  onDraftChange: (String) -> Unit,
  onApply: (String) -> Unit,
  onCancel: () -> Unit,
) {
  if (visible) {
    Box(modifier = Modifier.fillMaxSize()) {
      DeviceIdBadge(
        hardwareId = hardwareId,
        onClick = onOpenEditor,
        modifier = Modifier.align(Alignment.TopEnd).padding(16.dp),
      )
    }
  }
  if (editing) {
    Box(
      modifier =
        Modifier
          .fillMaxSize()
          .background(Color(0xB305070E))
          .clickable(onClick = onCancel),
      contentAlignment = Alignment.Center,
    ) {
      Box(modifier = Modifier.clickable(enabled = false, onClick = {})) {
        DeviceIdDialog(
          draftDeviceId = draftDeviceId,
          onDraftChange = onDraftChange,
          onApply = onApply,
          onCancel = onCancel,
        )
      }
    }
  }
}

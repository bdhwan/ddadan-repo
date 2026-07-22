package com.ddadan.player

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ddadan.player.BuildConfig
import com.ddadan.core.ServerLocator
import com.ddadan.player.data.PlayerRepository
import com.ddadan.player.data.ScreenResponse
import com.ddadan.player.data.SlidePayload
import com.ddadan.player.prefs.PlayerPreferences
import com.ddadan.player.util.buildRotationKey
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlin.coroutines.coroutineContext

data class PlayerUiState(
  val screen: ScreenResponse? = null,
  val hardwareId: String = "dev-local",
  val slot: Int = 0,
  val apiBase: String = BuildConfig.API_BASE,
  val isLoading: Boolean = true,
  val rotationSlides: List<SlidePayload>? = null,
  val rotIdx0: Int = 0,
  val rotIdx1: Int = 1,
  val rotOp0: Float = 1f,
  val rotOp1: Float = 0f,
  val rotTransition: Boolean = false,
  val editingDeviceId: Boolean = false,
  val draftDeviceId: String = "",
  val editingSettings: Boolean = false,
  val draftApiBase: String = "",
  val draftSlot: String = "0",
  // 서버 자동 탐색 상태
  val discovering: Boolean = false,
  val scanCurrentIp: String? = null,
  val scanDone: Int = 0,
  val scanTotal: Int = 0,
  val retryCountdownSec: Int = 0,
)

class PlayerViewModel(
  private val repository: PlayerRepository,
  private val preferences: PlayerPreferences,
) : ViewModel() {
  private val _uiState = MutableStateFlow(PlayerUiState())
  val uiState: StateFlow<PlayerUiState> = _uiState.asStateFlow()

  private val locator = ServerLocator()

  private var pollJob: Job? = null
  private var rotationJob: Job? = null
  private var lastRotationKey = ""

  /** 사용자가 수동으로 API 주소를 지정한 상태인지. true면 자동 탐색을 하지 않는다. */
  private var overridePresent = false

  fun start() {
    if (pollJob?.isActive == true) return
    viewModelScope.launch {
      preferences.config.collect { config ->
        overridePresent = config.apiBaseOverride != null
        _uiState.update {
          it.copy(
            hardwareId = config.deviceId,
            slot = config.slot,
            apiBase = config.effectiveApiBase,
          )
        }
        restartPolling()
      }
    }
  }

  fun seedFromIntent(deviceId: String?, slot: Int?, apiBase: String?) {
    viewModelScope.launch {
      preferences.seedFromIntent(deviceId, slot, apiBase)
    }
  }

  private fun restartPolling() {
    pollJob?.cancel()
    pollJob =
      viewModelScope.launch {
        while (isActive) {
          val ok = fetchOnce()
          if (ok || overridePresent) {
            // 정상 응답이거나 수동 지정 모드면 평소대로 폴링만 반복.
            delay(BuildConfig.POLL_INTERVAL_MS)
          } else {
            // 자동 모드에서 응답이 없으면 서버를 찾을 때까지 탐색한 뒤 폴링 재개.
            runDiscoveryUntilFound()
          }
        }
      }
  }

  /** @return 화면 응답을 정상적으로 받았으면 true. */
  private suspend fun fetchOnce(): Boolean {
    val state = _uiState.value
    val hwid = state.hardwareId
    if (hwid.isBlank()) return false
    return try {
      val response = repository.fetchScreen(hwid, state.slot)
      applyResponse(response)
      true
    } catch (e: Exception) {
      android.util.Log.w("PlayerViewModel", "player fetch failed: ${e.message}")
      false
    } finally {
      _uiState.update { it.copy(isLoading = false) }
    }
  }

  /**
   * 로컬 네트워크를 스캔해 서버를 찾을 때까지 반복한다.
   * 한 바퀴 다 돌아도 못 찾으면 3분 카운트다운 후 재시도. 찾으면 주소를 저장하고 반환.
   */
  private suspend fun runDiscoveryUntilFound() {
    _uiState.update {
      it.copy(discovering = true, scanCurrentIp = null, scanDone = 0, scanTotal = 0, retryCountdownSec = 0)
    }
    try {
      while (coroutineContext.isActive) {
        _uiState.update { it.copy(scanCurrentIp = null, scanDone = 0, scanTotal = 0, retryCountdownSec = 0) }

        val found =
          locator.scanOnce { progress ->
            _uiState.update {
              it.copy(
                scanCurrentIp = progress.currentIp,
                scanDone = progress.done,
                scanTotal = progress.total,
              )
            }
          }

        if (found != null) {
          preferences.setDiscoveredApiBase(found)
          _uiState.update {
            it.copy(discovering = false, scanCurrentIp = null, apiBase = found)
          }
          return
        }

        // 못 찾음 → 3분 대기(카운트다운) 후 다시 스캔.
        for (sec in RETRY_DELAY_SEC downTo 1) {
          if (!coroutineContext.isActive) return
          _uiState.update { it.copy(retryCountdownSec = sec, scanCurrentIp = null) }
          delay(1000)
        }
      }
    } finally {
      _uiState.update { it.copy(discovering = false, retryCountdownSec = 0) }
    }
  }

  private fun applyResponse(res: ScreenResponse) {
    _uiState.update { it.copy(screen = res, isLoading = false) }
    val slides = res.rotation?.slides
    if (res.mode == "rotation" && slides != null && slides.size >= 2) {
      val key =
        buildRotationKey(
          slides,
          res.rotation.intervalMs,
          res.rotation.fadeMs,
        )
      if (key != lastRotationKey) {
        lastRotationKey = key
        rotationJob?.cancel()
        // 페이드 사이에는 오버레이(idx1)가 베이스(idx0)와 같은 슬라이드를 가리킨다 →
        // 오버레이가 숨김 상태에서 잠깐 보여도 베이스와 동일해 튕김/잔상이 없다.
        _uiState.update {
          it.copy(
            rotationSlides = slides,
            rotIdx0 = 0,
            rotIdx1 = 0,
            rotOp0 = 1f,
            rotOp1 = 0f,
            rotTransition = false,
          )
        }
        scheduleRotationStep(res.rotation.intervalMs, res.rotation.fadeMs)
      }
    } else {
      lastRotationKey = ""
      rotationJob?.cancel()
      _uiState.update { it.copy(rotationSlides = null) }
    }
  }

  private fun scheduleRotationStep(intervalMs: Long, fadeMs: Long) {
    rotationJob?.cancel()
    rotationJob =
      viewModelScope.launch {
        delay(intervalMs)
        val slides = _uiState.value.rotationSlides ?: return@launch
        val n = slides.size
        val nextIdx = (_uiState.value.rotIdx0 + 1) % n
        // 오버레이(idx1)를 다음 슬라이드로 지정하고 0→1로 페이드 인(베이스는 계속 불투명).
        _uiState.update {
          it.copy(rotIdx1 = nextIdx, rotTransition = true, rotOp0 = 0f, rotOp1 = 1f)
        }
        delay(fadeMs)
        // 페이드 완료: 베이스를 다음 슬라이드로 승격. 오버레이는 그대로 다음 슬라이드를 가리킨
        // 채 숨긴다(베이스와 동일 → 오버레이 alpha가 늦게 스냅돼도 튕김 없음).
        _uiState.update {
          it.copy(rotIdx0 = nextIdx, rotTransition = false, rotOp0 = 1f, rotOp1 = 0f)
        }
        scheduleRotationStep(intervalMs, fadeMs)
      }
  }

  val needsRegister: Boolean
    get() {
      val s = _uiState.value.screen ?: return false
      return !s.registered || s.isFallback == true
    }

  fun openDeviceIdEditor() {
    _uiState.update {
      it.copy(editingDeviceId = true, draftDeviceId = it.hardwareId)
    }
  }

  fun cancelDeviceIdEditor() {
    _uiState.update { it.copy(editingDeviceId = false) }
  }

  fun applyDeviceId(value: String) {
    val next = value.trim()
    if (next.isBlank()) return
    viewModelScope.launch {
      preferences.setDeviceId(next)
      _uiState.update { it.copy(editingDeviceId = false) }
      fetchOnce()
    }
  }

  fun openSettingsEditor() {
    _uiState.update {
      it.copy(
        editingSettings = true,
        draftApiBase = it.apiBase,
        draftSlot = it.slot.toString(),
      )
    }
  }

  fun cancelSettingsEditor() {
    _uiState.update { it.copy(editingSettings = false) }
  }

  fun applySettings(apiBase: String, slotText: String) {
    val slot = slotText.trim().toIntOrNull() ?: 0
    viewModelScope.launch {
      val trimmedApi = apiBase.trim()
      if (trimmedApi.isBlank() || trimmedApi == BuildConfig.API_BASE) {
        preferences.setApiBaseOverride(null)
      } else {
        preferences.setApiBaseOverride(trimmedApi)
      }
      preferences.setSlot(slot)
      _uiState.update { it.copy(editingSettings = false) }
      fetchOnce()
    }
  }

  fun resetApiBase() {
    viewModelScope.launch {
      preferences.setApiBaseOverride(null)
      _uiState.update { it.copy(editingSettings = false) }
      fetchOnce()
    }
  }

  fun updateDraftDeviceId(value: String) {
    _uiState.update { it.copy(draftDeviceId = value) }
  }

  fun updateDraftApiBase(value: String) {
    _uiState.update { it.copy(draftApiBase = value) }
  }

  fun updateDraftSlot(value: String) {
    _uiState.update { it.copy(draftSlot = value) }
  }

  override fun onCleared() {
    pollJob?.cancel()
    rotationJob?.cancel()
    super.onCleared()
  }

  companion object {
    /** 서버를 못 찾았을 때 다음 스캔까지 대기 시간(초). */
    private const val RETRY_DELAY_SEC = 180
  }
}

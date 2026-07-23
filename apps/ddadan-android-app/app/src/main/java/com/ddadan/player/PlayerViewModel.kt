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
import kotlinx.serialization.json.Json
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
  // 콘텐츠는 있는데 서버 폴링이 연속 실패 중(탐색 진입 전 유예 구간). 화면 유지 + 배지.
  val reconnecting: Boolean = false,
  // 부팅 직후 네트워크(Wi-Fi/IP) 연결을 기다리는 중.
  val awaitingNetwork: Boolean = false,
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
  private var lastCachedJson = ""

  private val json = Json { ignoreUnknownKeys = true; isLenient = true; encodeDefaults = true }

  /** 사용자가 수동으로 API 주소를 지정한 상태인지. true면 자동 탐색을 하지 않는다. */
  private var overridePresent = false

  fun start() {
    if (pollJob?.isActive == true) return
    viewModelScope.launch {
      // 1) 캐시된 마지막 화면을 먼저 띄운다 — 네트워크가 붙기 전에도 곧바로 콘텐츠가 보인다.
      restoreCachedScreen()
      // 2) 네트워크(IP 할당)까지 대기. 부팅 직후 Wi-Fi 전에 서버 접속하면 무조건 실패하므로.
      _uiState.update { it.copy(awaitingNetwork = true) }
      preferences.awaitNetworkReady()
      _uiState.update { it.copy(awaitingNetwork = false) }
      // 3) 설정을 구독하며 폴링 시작.
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

  /**
   * 방금 받은 화면을 캐시에 저장한다(다음 부팅 때 먼저 보여줄 용도).
   * 내용이 바뀌었을 때만 기록해 5초 폴링마다 플래시에 쓰지 않는다.
   */
  private fun cacheScreen(res: ScreenResponse) {
    val enc =
      try {
        json.encodeToString(ScreenResponse.serializer(), res)
      } catch (e: Exception) {
        return
      }
    if (enc == lastCachedJson) return
    lastCachedJson = enc
    viewModelScope.launch {
      try {
        preferences.setLastScreen(enc)
      } catch (e: Exception) {
        android.util.Log.w("PlayerViewModel", "cache write failed: ${e.message}")
      }
    }
  }

  /** 저장된 마지막 화면(JSON)이 있으면 복원해 즉시 표시한다. */
  private suspend fun restoreCachedScreen() {
    val cached = preferences.getLastScreen() ?: return
    try {
      val screen = json.decodeFromString(ScreenResponse.serializer(), cached)
      lastCachedJson = cached
      applyResponse(screen)
    } catch (e: Exception) {
      android.util.Log.w("PlayerViewModel", "cached screen restore failed: ${e.message}")
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
        // 서버가 잠깐 끊긴 것(api 재시작·순단)만으로 곧장 전체 탐색에 들어가면, 잘 나오던
        // 메뉴가 성급하게 탐색 화면으로 바뀐다. 연속 실패가 임계값을 넘을 때만 탐색한다.
        var consecutiveFailures = 0
        while (isActive) {
          val ok = fetchOnce()
          when {
            ok -> {
              // 정상 응답 — 실패 카운터/재연결 배지 해제.
              if (consecutiveFailures != 0) {
                consecutiveFailures = 0
                _uiState.update { it.copy(reconnecting = false) }
              }
              delay(BuildConfig.POLL_INTERVAL_MS)
            }
            overridePresent -> {
              // 수동 지정 모드: 탐색하지 않고 같은 주소로 계속 재시도.
              delay(BuildConfig.POLL_INTERVAL_MS)
            }
            else -> {
              consecutiveFailures++
              // 이미 보여줄 콘텐츠가 있으면 화면은 그대로 두고, 좌상단에 재연결 배지만 띄운다.
              if (_uiState.value.screen != null) {
                _uiState.update { it.copy(reconnecting = true) }
              }
              if (consecutiveFailures >= MAX_CONSECUTIVE_FAILURES) {
                // 충분히 오래(≈MAX×폴링간격) 못 붙었으면 서버를 다시 탐색.
                _uiState.update { it.copy(reconnecting = false) }
                runDiscoveryUntilFound()
                consecutiveFailures = 0
              } else {
                delay(BuildConfig.POLL_INTERVAL_MS)
              }
            }
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
   * 한 바퀴 다 돌아도 못 찾으면 30초 카운트다운 후 재시도. 찾으면 주소를 저장하고 반환.
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

        // 못 찾음 → 30초 대기(카운트다운) 후 다시 스캔.
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
    cacheScreen(res)
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
    private const val RETRY_DELAY_SEC = 30

    /**
     * 자동 모드에서 폴링이 이만큼 연속 실패해야 전체 탐색에 들어간다.
     * 폴링 간격 5초 × 10 ≈ 50초 — 짧은 서버 순단/재시작은 화면을 안 건드리고 넘어간다.
     */
    private const val MAX_CONSECUTIVE_FAILURES = 10
  }
}

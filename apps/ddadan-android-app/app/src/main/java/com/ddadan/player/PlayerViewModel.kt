package com.ddadan.player

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ddadan.player.BuildConfig
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
)

class PlayerViewModel(
  private val repository: PlayerRepository,
  private val preferences: PlayerPreferences,
) : ViewModel() {
  private val _uiState = MutableStateFlow(PlayerUiState())
  val uiState: StateFlow<PlayerUiState> = _uiState.asStateFlow()

  private var pollJob: Job? = null
  private var rotationJob: Job? = null
  private var lastRotationKey = ""

  fun start() {
    if (pollJob?.isActive == true) return
    viewModelScope.launch {
      preferences.config.collect { config ->
        val apiBase = config.apiBaseOverride ?: BuildConfig.API_BASE
        _uiState.update {
          it.copy(
            hardwareId = config.deviceId,
            slot = config.slot,
            apiBase = apiBase,
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
          fetchOnce()
          delay(BuildConfig.POLL_INTERVAL_MS)
        }
      }
  }

  private suspend fun fetchOnce() {
    val state = _uiState.value
    val hwid = state.hardwareId
    if (hwid.isBlank()) return
    try {
      val response = repository.fetchScreen(hwid, state.slot)
      applyResponse(response)
    } catch (e: Exception) {
      android.util.Log.w("PlayerViewModel", "player fetch failed: ${e.message}")
    } finally {
      _uiState.update { it.copy(isLoading = false) }
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
        _uiState.update {
          it.copy(
            rotationSlides = slides,
            rotIdx0 = 0,
            rotIdx1 = 1,
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
        _uiState.update {
          it.copy(rotTransition = true, rotOp0 = 0f, rotOp1 = 1f)
        }
        delay(fadeMs)
        val slides = _uiState.value.rotationSlides ?: return@launch
        val n = slides.size
        val nextTop = (_uiState.value.rotIdx1 + 1) % n
        _uiState.update {
          it.copy(
            rotIdx0 = it.rotIdx1,
            rotIdx1 = nextTop,
            rotTransition = false,
            rotOp0 = 1f,
            rotOp1 = 0f,
          )
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
}

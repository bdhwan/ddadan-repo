package com.ddadan.player.prefs

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import com.ddadan.core.DeviceIdentity
import com.ddadan.player.BuildConfig
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

private val Context.playerDataStore: DataStore<Preferences> by preferencesDataStore(name = "ddadan_player")

data class PlayerConfig(
  val deviceId: String = "dev-local",
  val slot: Int = 0,
  val apiBaseOverride: String? = null,
  val discoveredApiBase: String? = null,
) {
  /**
   * 실제 사용할 API 기준 주소.
   * 우선순위: 사용자가 수동 지정한 override > 자동 탐색으로 찾은 주소 > 빌드 기본값.
   */
  val effectiveApiBase: String
    get() = apiBaseOverride ?: discoveredApiBase ?: BuildConfig.API_BASE
}

class PlayerPreferences(private val context: Context) {
  private val deviceIdKey = stringPreferencesKey("device_id")
  private val slotKey = intPreferencesKey("slot")
  private val apiBaseKey = stringPreferencesKey("api_base_override")
  private val discoveredApiBaseKey = stringPreferencesKey("discovered_api_base")

  // 박스별 고유 기본 deviceId(= 워치독과 동일 로직). 최초 1회 su 호출이라 캐시.
  private val defaultDeviceId: String by lazy { DeviceIdentity.hardwareId(context) }

  val config: Flow<PlayerConfig> =
    context.playerDataStore.data.map { prefs ->
      PlayerConfig(
        deviceId = prefs[deviceIdKey] ?: defaultDeviceId,
        slot = prefs[slotKey] ?: 0,
        apiBaseOverride = prefs[apiBaseKey]?.takeIf { it.isNotBlank() },
        discoveredApiBase = prefs[discoveredApiBaseKey]?.takeIf { it.isNotBlank() },
      )
    }

  suspend fun setDeviceId(value: String) {
    context.playerDataStore.edit { it[deviceIdKey] = value }
  }

  suspend fun setSlot(value: Int) {
    context.playerDataStore.edit { it[slotKey] = value }
  }

  suspend fun setApiBaseOverride(value: String?) {
    context.playerDataStore.edit {
      if (value.isNullOrBlank()) {
        it.remove(apiBaseKey)
      } else {
        it[apiBaseKey] = value.trim()
      }
    }
  }

  suspend fun setDiscoveredApiBase(value: String?) {
    context.playerDataStore.edit {
      if (value.isNullOrBlank()) {
        it.remove(discoveredApiBaseKey)
      } else {
        it[discoveredApiBaseKey] = value.trim()
      }
    }
  }

  suspend fun seedFromIntent(deviceId: String?, slot: Int?, apiBase: String?) {
    context.playerDataStore.edit { prefs ->
      if (!deviceId.isNullOrBlank() && prefs[deviceIdKey] == null) {
        prefs[deviceIdKey] = deviceId.trim()
      }
      if (slot != null && prefs[slotKey] == null) {
        prefs[slotKey] = slot
      }
      if (!apiBase.isNullOrBlank() && prefs[apiBaseKey] == null) {
        prefs[apiBaseKey] = apiBase.trim()
      }
    }
  }
}

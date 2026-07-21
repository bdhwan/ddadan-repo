package com.ddadan.core

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map

private val Context.coreDataStore: DataStore<Preferences> by preferencesDataStore(name = "ddadan_core")

data class CoreSettings(
  val apiBaseOverride: String? = null,
  val discoveredApiBase: String? = null,
  val hardwareIdOverride: String? = null,
) {
  /** 실제 사용할 API 기준 주소. override → discovered → 빌드 기본값. */
  val effectiveApiBase: String
    get() = apiBaseOverride ?: discoveredApiBase ?: BuildConfig.API_BASE
}

/**
 * 워치독 앱의 설정 저장소(플레이어의 PlayerPreferences와 동일 개념, 별도 DataStore).
 * hardwareId는 저장하지 않고 [DeviceIdentity]로 계산 — 같은 박스의 두 앱이 동일값을 얻도록.
 */
class CoreConfig(private val context: Context) {
  private val apiBaseKey = stringPreferencesKey("api_base_override")
  private val discoveredKey = stringPreferencesKey("discovered_api_base")
  private val hardwareIdKey = stringPreferencesKey("hardware_id_override")

  val settings: Flow<CoreSettings> =
    context.coreDataStore.data.map { prefs ->
      CoreSettings(
        apiBaseOverride = prefs[apiBaseKey]?.takeIf { it.isNotBlank() },
        discoveredApiBase = prefs[discoveredKey]?.takeIf { it.isNotBlank() },
        hardwareIdOverride = prefs[hardwareIdKey]?.takeIf { it.isNotBlank() },
      )
    }

  suspend fun effectiveApiBase(): String = settings.first().effectiveApiBase

  suspend fun hardwareId(): String =
    settings.first().hardwareIdOverride ?: DeviceIdentity.hardwareId(context)

  suspend fun setDiscoveredApiBase(value: String?) {
    context.coreDataStore.edit {
      if (value.isNullOrBlank()) it.remove(discoveredKey) else it[discoveredKey] = value.trim()
    }
  }

  suspend fun setApiBaseOverride(value: String?) {
    context.coreDataStore.edit {
      if (value.isNullOrBlank()) it.remove(apiBaseKey) else it[apiBaseKey] = value.trim()
    }
  }

  suspend fun setHardwareIdOverride(value: String?) {
    context.coreDataStore.edit {
      if (value.isNullOrBlank()) it.remove(hardwareIdKey) else it[hardwareIdKey] = value.trim()
    }
  }
}

package com.ddadan.player.prefs

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

private val Context.playerDataStore: DataStore<Preferences> by preferencesDataStore(name = "ddadan_player")

data class PlayerConfig(
  val deviceId: String = "dev-local",
  val slot: Int = 0,
  val apiBaseOverride: String? = null,
)

class PlayerPreferences(private val context: Context) {
  private val deviceIdKey = stringPreferencesKey("device_id")
  private val slotKey = intPreferencesKey("slot")
  private val apiBaseKey = stringPreferencesKey("api_base_override")

  val config: Flow<PlayerConfig> =
    context.playerDataStore.data.map { prefs ->
      PlayerConfig(
        deviceId = prefs[deviceIdKey] ?: "dev-local",
        slot = prefs[slotKey] ?: 0,
        apiBaseOverride = prefs[apiBaseKey]?.takeIf { it.isNotBlank() },
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

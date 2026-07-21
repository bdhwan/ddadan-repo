package com.ddadan.player.data

import com.ddadan.player.BuildConfig
import com.ddadan.player.prefs.PlayerPreferences
import retrofit2.converter.kotlinx.serialization.asConverterFactory
import kotlinx.coroutines.flow.first
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.logging.HttpLoggingInterceptor
import retrofit2.Retrofit
import java.util.concurrent.TimeUnit

class PlayerRepository(
  private val preferences: PlayerPreferences,
) {
  private val json =
    Json {
      ignoreUnknownKeys = true
      isLenient = true
    }

  suspend fun effectiveApiBase(): String = preferences.config.first().effectiveApiBase

  suspend fun fetchScreen(hardwareId: String, slot: Int): ScreenResponse {
    val apiBase = effectiveApiBase()
    val api = createApi(apiBase)
    return api.getScreen(hardwareId, slot)
  }

  private fun createApi(apiBase: String): PlayerApi {
    val normalizedBase = if (apiBase.endsWith("/")) apiBase else "$apiBase/"
    val clientBuilder =
      OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(15, TimeUnit.SECONDS)
        .writeTimeout(30, TimeUnit.SECONDS)

    if (BuildConfig.DEBUG) {
      clientBuilder.addInterceptor(
        HttpLoggingInterceptor().apply { level = HttpLoggingInterceptor.Level.BASIC },
      )
    }

    val retrofit =
      Retrofit.Builder()
        .baseUrl(normalizedBase)
        .client(clientBuilder.build())
        .addConverterFactory(json.asConverterFactory("application/json".toMediaType()))
        .build()

    return retrofit.create(PlayerApi::class.java)
  }
}

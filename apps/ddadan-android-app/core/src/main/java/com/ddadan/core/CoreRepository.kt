package com.ddadan.core

import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.RequestBody.Companion.toRequestBody
import retrofit2.Retrofit
import retrofit2.converter.kotlinx.serialization.asConverterFactory
import java.util.concurrent.TimeUnit

/** 워치독이 서버와 통신하는 저장소(플레이어 PlayerRepository와 동일 패턴). */
class CoreRepository(private val config: CoreConfig) {
  private val json = Json { ignoreUnknownKeys = true; isLenient = true }

  suspend fun effectiveApiBase(): String = config.effectiveApiBase()

  /** API 기준 주소에서 `/api`를 뗀 origin(정적 파일 URL 조합용). */
  suspend fun apiOrigin(): String = effectiveApiBase().trimEnd('/').removeSuffix("/api")

  suspend fun uploadScreenshot(hardwareId: String, jpeg: ByteArray) {
    val body = jpeg.toRequestBody("image/jpeg".toMediaType())
    val part = MultipartBody.Part.createFormData("file", "screen.jpg", body)
    createApi(effectiveApiBase()).uploadScreenshot(hardwareId, part)
  }

  suspend fun postTelemetry(hardwareId: String, body: TelemetryBody) {
    createApi(effectiveApiBase()).postTelemetry(hardwareId, body)
  }

  suspend fun getLatestApk(applicationId: String? = null): ApkInfo =
    createApi(effectiveApiBase()).getLatestApk(applicationId)

  private fun createApi(apiBase: String): CoreApi {
    val normalized = if (apiBase.endsWith("/")) apiBase else "$apiBase/"
    val client =
      OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(15, TimeUnit.SECONDS)
        .writeTimeout(30, TimeUnit.SECONDS)
        .build()
    return Retrofit.Builder()
      .baseUrl(normalized)
      .client(client)
      .addConverterFactory(json.asConverterFactory("application/json".toMediaType()))
      .build()
      .create(CoreApi::class.java)
  }
}

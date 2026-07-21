package com.ddadan.core

import okhttp3.MultipartBody
import retrofit2.http.Body
import retrofit2.http.GET
import retrofit2.http.Multipart
import retrofit2.http.POST
import retrofit2.http.Part
import retrofit2.http.Path
import retrofit2.http.Query

interface CoreApi {
  @Multipart
  @POST("devices/{hardwareId}/screenshots")
  suspend fun uploadScreenshot(
    @Path("hardwareId") hardwareId: String,
    @Part file: MultipartBody.Part,
  ): UploadAck

  @GET("apks/latest")
  suspend fun getLatestApk(
    @Query("applicationId") applicationId: String? = null,
  ): ApkInfo

  @POST("devices/{hardwareId}/telemetry")
  suspend fun postTelemetry(
    @Path("hardwareId") hardwareId: String,
    @Body body: TelemetryBody,
  ): UploadAck

  @GET("devices/{hardwareId}/commands/pending")
  suspend fun getPendingCommands(
    @Path("hardwareId") hardwareId: String,
  ): List<DeviceCommandDto>

  @POST("devices/{hardwareId}/commands/{cmdId}/ack")
  suspend fun ackCommand(
    @Path("hardwareId") hardwareId: String,
    @Path("cmdId") cmdId: Int,
    @Body body: AckBody,
  ): UploadAck
}

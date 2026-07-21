package com.ddadan.player.data

import okhttp3.MultipartBody
import retrofit2.http.GET
import retrofit2.http.Multipart
import retrofit2.http.POST
import retrofit2.http.Part
import retrofit2.http.Path
import retrofit2.http.Query

interface PlayerApi {
  @GET("player/{hardwareId}/screen")
  suspend fun getScreen(
    @Path("hardwareId") hardwareId: String,
    @Query("slot") slot: Int,
  ): ScreenResponse

  @Multipart
  @POST("devices/{hardwareId}/screenshots")
  suspend fun uploadScreenshot(
    @Path("hardwareId") hardwareId: String,
    @Part file: MultipartBody.Part,
  ): UploadAck

  @GET("apks/latest")
  suspend fun getLatestApk(): ApkInfo
}

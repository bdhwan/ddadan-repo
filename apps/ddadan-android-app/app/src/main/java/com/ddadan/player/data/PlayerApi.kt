package com.ddadan.player.data

import retrofit2.http.GET
import retrofit2.http.Path
import retrofit2.http.Query

interface PlayerApi {
  @GET("player/{hardwareId}/screen")
  suspend fun getScreen(
    @Path("hardwareId") hardwareId: String,
    @Query("slot") slot: Int,
  ): ScreenResponse
}

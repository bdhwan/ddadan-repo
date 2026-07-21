package com.ddadan.player

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import com.ddadan.player.data.PlayerRepository
import com.ddadan.player.prefs.PlayerPreferences

class PlayerViewModelFactory(
  private val repository: PlayerRepository,
  private val preferences: PlayerPreferences,
) : ViewModelProvider.Factory {
  @Suppress("UNCHECKED_CAST")
  override fun <T : ViewModel> create(modelClass: Class<T>): T {
    if (modelClass.isAssignableFrom(PlayerViewModel::class.java)) {
      return PlayerViewModel(repository, preferences) as T
    }
    throw IllegalArgumentException("Unknown ViewModel class")
  }
}

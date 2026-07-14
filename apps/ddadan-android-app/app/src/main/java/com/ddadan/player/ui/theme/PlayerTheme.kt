package com.ddadan.player.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val KioskColorScheme =
  darkColorScheme(
    background = Color(0xFF050505),
    surface = Color(0xFF0C0F1A),
    onBackground = Color.White,
    onSurface = Color.White,
    primary = Color(0xFF2C7BE5),
  )

@Composable
fun DDADANPlayerTheme(content: @Composable () -> Unit) {
  MaterialTheme(colorScheme = KioskColorScheme, content = content)
}

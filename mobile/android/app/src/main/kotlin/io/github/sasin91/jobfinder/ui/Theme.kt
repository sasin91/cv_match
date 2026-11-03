package io.github.sasin91.jobfinder.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// The computer app's colours (app/src/desktop/style.rs): oat paper by day, the A4 evening by night.

private val Light = lightColorScheme(
    primary = Color(0xFF4F6D4D),
    onPrimary = Color(0xFFFCF9F3),
    primaryContainer = Color(0xFFE1E9DC),
    onPrimaryContainer = Color(0xFF2A231C),
    secondary = Color(0xFF8A4A2F),
    onSecondary = Color(0xFFFCF9F3),
    background = Color(0xFFF5EFE4),
    onBackground = Color(0xFF2A231C),
    surface = Color(0xFFFCF9F3),
    onSurface = Color(0xFF2A231C),
    surfaceVariant = Color(0xFFECE4D6),
    onSurfaceVariant = Color(0xFF6F6456),
    surfaceContainer = Color(0xFFF5EFE4),
    surfaceContainerLow = Color(0xFFFCF9F3),
    outline = Color(0xFFD4C7B2),
    outlineVariant = Color(0xFFE2D7C4),
    error = Color(0xFFB23A26),
    onError = Color(0xFFFCF9F3),
)

private val Dark = darkColorScheme(
    primary = Color(0xFF9DBF9A),
    onPrimary = Color(0xFF221B16),
    primaryContainer = Color(0xFF2F3A2C),
    onPrimaryContainer = Color(0xFFF1E6D6),
    secondary = Color(0xFFD9A07F),
    onSecondary = Color(0xFF221B16),
    background = Color(0xFF221B16),
    onBackground = Color(0xFFF1E6D6),
    surface = Color(0xFF2D241E),
    onSurface = Color(0xFFF1E6D6),
    surfaceVariant = Color(0xFF3A2F27),
    onSurfaceVariant = Color(0xFFB8A792),
    surfaceContainer = Color(0xFF221B16),
    surfaceContainerLow = Color(0xFF2D241E),
    outline = Color(0xFF5A4A3D),
    outlineVariant = Color(0xFF43362C),
    error = Color(0xFFE07A66),
    onError = Color(0xFF221B16),
)

/** A verdict's or a tag's colour: sage for met and top, ochre for partial, clay for missing. */
object Marks {
    @Composable
    fun of(kind: String): Color {
        val dark = isSystemInDarkTheme()
        return when (kind) {
            "met", "top", "good" -> if (dark) Color(0xFF9DBF9A) else Color(0xFF4F6D4D)
            "partial", "fair" -> if (dark) Color(0xFFE0B66A) else Color(0xFF8A5A12)
            "missing", "low" -> if (dark) Color(0xFFE07A66) else Color(0xFFB23A26)
            else -> MaterialTheme.colorScheme.onSurfaceVariant
        }
    }
}

@Composable
fun JobFinderTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, content = content)
}

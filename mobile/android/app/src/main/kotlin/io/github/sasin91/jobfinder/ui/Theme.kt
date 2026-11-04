package io.github.sasin91.jobfinder.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import io.github.sasin91.jobfinder.R

// The computer app's colours (app/src/desktop/style.rs): oat paper by day, the A4 evening by night.
// Links are rust (secondary), as the desktop's and the web page's.

private val Light = lightColorScheme(
    primary = Color(0xFF4F6D4D),
    onPrimary = Color(0xFFFCF9F3),
    primaryContainer = Color(0xFFE1E9DC),
    onPrimaryContainer = Color(0xFF2A231C),
    secondary = Color(0xFF8A4A2F),
    onSecondary = Color(0xFFFCF9F3),
    // A selected chip and the bottom bar's pill: the sage accent, as the desktop's selected items.
    secondaryContainer = Color(0xFF4F6D4D),
    onSecondaryContainer = Color(0xFFFCF9F3),
    tertiary = Color(0xFF8A4A2F),
    onTertiary = Color(0xFFFCF9F3),
    tertiaryContainer = Color(0xFFECE4D6),
    onTertiaryContainer = Color(0xFF2A231C),
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
    secondary = Color(0xFFE6A46A),
    onSecondary = Color(0xFF221B16),
    secondaryContainer = Color(0xFF9DBF9A),
    onSecondaryContainer = Color(0xFF221B16),
    tertiary = Color(0xFFE6A46A),
    onTertiary = Color(0xFF221B16),
    tertiaryContainer = Color(0xFF3A2F27),
    onTertiaryContainer = Color(0xFFF1E6D6),
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
            "partial", "medium", "fair" -> if (dark) Color(0xFFE0B66A) else Color(0xFF8A5A12)
            "missing", "poor", "low" -> if (dark) Color(0xFFE07A66) else Color(0xFFB23A26)
            else -> MaterialTheme.colorScheme.onSurfaceVariant
        }
    }
}

// The desktop's and the web page's type: Newsreader for titles, Figtree for the rest (OFL, from
// assets/fonts at the repository's root).
private val Newsreader = FontFamily(Font(R.font.newsreader_regular, FontWeight.Normal))
private val Figtree = FontFamily(
    Font(R.font.figtree_regular, FontWeight.Normal),
    Font(R.font.figtree_semibold, FontWeight.SemiBold),
)

private val Typography = Typography().let { base ->
    base.copy(
        displayLarge = base.displayLarge.copy(fontFamily = Newsreader),
        displayMedium = base.displayMedium.copy(fontFamily = Newsreader),
        displaySmall = base.displaySmall.copy(fontFamily = Newsreader),
        headlineLarge = base.headlineLarge.copy(fontFamily = Newsreader),
        headlineMedium = base.headlineMedium.copy(fontFamily = Newsreader),
        headlineSmall = base.headlineSmall.copy(fontFamily = Newsreader),
        titleLarge = base.titleLarge.copy(fontFamily = Newsreader),
        titleMedium = base.titleMedium.copy(fontFamily = Figtree, fontWeight = FontWeight.SemiBold),
        titleSmall = base.titleSmall.copy(fontFamily = Figtree, fontWeight = FontWeight.SemiBold),
        bodyLarge = base.bodyLarge.copy(fontFamily = Figtree),
        bodyMedium = base.bodyMedium.copy(fontFamily = Figtree),
        bodySmall = base.bodySmall.copy(fontFamily = Figtree),
        labelLarge = base.labelLarge.copy(fontFamily = Figtree, fontWeight = FontWeight.SemiBold),
        labelMedium = base.labelMedium.copy(fontFamily = Figtree, fontWeight = FontWeight.SemiBold),
        labelSmall = base.labelSmall.copy(fontFamily = Figtree, fontWeight = FontWeight.SemiBold),
    )
}

@Composable
fun JobFinderTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) Dark else Light,
        typography = Typography,
        content = content,
    )
}

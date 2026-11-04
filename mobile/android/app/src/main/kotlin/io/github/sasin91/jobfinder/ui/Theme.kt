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

// The computer app's colours (app/src/desktop/style.rs), the "Nordic" look: cool paper and navy
// ink by day, deep fjord by night, pine as the accent, fjord blue for links (secondary), marigold
// for warnings.

private val Light = lightColorScheme(
    primary = Color(0xFF2E6B5B),
    onPrimary = Color(0xFFF9FBFA),
    primaryContainer = Color(0xFFDCEBE5),
    onPrimaryContainer = Color(0xFF142430),
    secondary = Color(0xFF1F5C8A),
    onSecondary = Color(0xFFF9FBFA),
    // A selected chip and the bottom bar's pill: the pine accent, as the desktop's selected items.
    secondaryContainer = Color(0xFF2E6B5B),
    onSecondaryContainer = Color(0xFFF9FBFA),
    tertiary = Color(0xFF1F5C8A),
    onTertiary = Color(0xFFF9FBFA),
    tertiaryContainer = Color(0xFFE2E8E6),
    onTertiaryContainer = Color(0xFF142430),
    background = Color(0xFFEEF1EF),
    onBackground = Color(0xFF142430),
    surface = Color(0xFFF9FBFA),
    onSurface = Color(0xFF142430),
    surfaceVariant = Color(0xFFE2E8E6),
    onSurfaceVariant = Color(0xFF526270),
    surfaceContainer = Color(0xFFEEF1EF),
    surfaceContainerLow = Color(0xFFF9FBFA),
    outline = Color(0xFFD5DDDB),
    outlineVariant = Color(0xFFE1E7E5),
    error = Color(0xFFB23A26),
    onError = Color(0xFFF9FBFA),
)

private val Dark = darkColorScheme(
    primary = Color(0xFF6FB59C),
    onPrimary = Color(0xFF0F1B24),
    primaryContainer = Color(0xFF1D3A35),
    onPrimaryContainer = Color(0xFFE6EEF0),
    secondary = Color(0xFF7DB6E3),
    onSecondary = Color(0xFF0F1B24),
    secondaryContainer = Color(0xFF6FB59C),
    onSecondaryContainer = Color(0xFF0F1B24),
    tertiary = Color(0xFF7DB6E3),
    onTertiary = Color(0xFF0F1B24),
    tertiaryContainer = Color(0xFF1E2F3A),
    onTertiaryContainer = Color(0xFFE6EEF0),
    background = Color(0xFF0F1B24),
    onBackground = Color(0xFFE6EEF0),
    surface = Color(0xFF16252F),
    onSurface = Color(0xFFE6EEF0),
    surfaceVariant = Color(0xFF1E2F3A),
    onSurfaceVariant = Color(0xFF93A6B1),
    surfaceContainer = Color(0xFF0F1B24),
    surfaceContainerLow = Color(0xFF16252F),
    outline = Color(0xFF263844),
    outlineVariant = Color(0xFF1E2F3A),
    error = Color(0xFFC4513A),
    onError = Color(0xFF0F1B24),
)

/** A verdict's or a tag's colour: pine for met and top, marigold for partial, red for missing. */
object Marks {
    @Composable
    fun of(kind: String): Color {
        val dark = isSystemInDarkTheme()
        return when (kind) {
            "met", "top", "good" -> if (dark) Color(0xFF6FB59C) else Color(0xFF2E6B5B)
            "partial", "medium", "fair" -> if (dark) Color(0xFFF0B952) else Color(0xFF8A5A12)
            "missing", "poor", "low" -> if (dark) Color(0xFFC4513A) else Color(0xFFB23A26)
            else -> MaterialTheme.colorScheme.onSurfaceVariant
        }
    }
}

// The desktop's and the web page's type: Bricolage Grotesque Bold for titles, Figtree for the rest
// (OFL, from assets/fonts at the repository's root).
private val Bricolage = FontFamily(Font(R.font.bricolage_grotesque_bold, FontWeight.Bold))
private val Figtree = FontFamily(
    Font(R.font.figtree_regular, FontWeight.Normal),
    Font(R.font.figtree_semibold, FontWeight.SemiBold),
)

private val Typography = Typography().let { base ->
    base.copy(
        displayLarge = base.displayLarge.copy(fontFamily = Bricolage, fontWeight = FontWeight.Bold),
        displayMedium = base.displayMedium.copy(fontFamily = Bricolage, fontWeight = FontWeight.Bold),
        displaySmall = base.displaySmall.copy(fontFamily = Bricolage, fontWeight = FontWeight.Bold),
        headlineLarge = base.headlineLarge.copy(fontFamily = Bricolage, fontWeight = FontWeight.Bold),
        headlineMedium = base.headlineMedium.copy(fontFamily = Bricolage, fontWeight = FontWeight.Bold),
        headlineSmall = base.headlineSmall.copy(fontFamily = Bricolage, fontWeight = FontWeight.Bold),
        titleLarge = base.titleLarge.copy(fontFamily = Bricolage, fontWeight = FontWeight.Bold),
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

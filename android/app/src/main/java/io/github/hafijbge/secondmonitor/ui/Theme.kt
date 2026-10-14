package io.github.hafijbge.secondmonitor.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Brand colours, matching the app icon. */
object Brand {
    val Blue = Color(0xFF3D7BFF)
    val Violet = Color(0xFF7A4DFF)
    val Indigo = Color(0xFF5A63FF)
    val gradient = Brush.linearGradient(listOf(Blue, Violet))
}

private val Light = lightColorScheme(
    primary = Brand.Indigo,
    onPrimary = Color.White,
    primaryContainer = Color(0xFFE2E4FF),
    onPrimaryContainer = Color(0xFF14185C),
    secondary = Color(0xFF5B5D72),
    secondaryContainer = Color(0xFFE0E1F9),
    tertiary = Brand.Violet,
    background = Color(0xFFF7F7FC),
    surface = Color(0xFFF7F7FC),
    surfaceContainerLowest = Color.White,
    surfaceContainerLow = Color(0xFFF1F1FA),
    surfaceContainer = Color(0xFFEBEBF5),
    surfaceContainerHigh = Color(0xFFE5E5F0),
    onSurface = Color(0xFF1A1B23),
    onSurfaceVariant = Color(0xFF5E6070),
    outlineVariant = Color(0xFFD6D7E4),
)

private val Dark = darkColorScheme(
    primary = Color(0xFFB9BEFF),
    onPrimary = Color(0xFF1E2384),
    primaryContainer = Color(0xFF3A42C9),
    onPrimaryContainer = Color(0xFFE2E4FF),
    secondary = Color(0xFFC4C5DD),
    secondaryContainer = Color(0xFF2E3046),
    tertiary = Color(0xFFCDBDFF),
    background = Color(0xFF0F1017),
    surface = Color(0xFF0F1017),
    surfaceContainerLowest = Color(0xFF0B0C12),
    surfaceContainerLow = Color(0xFF171822),
    surfaceContainer = Color(0xFF1C1D28),
    surfaceContainerHigh = Color(0xFF262734),
    onSurface = Color(0xFFE4E4F0),
    onSurfaceVariant = Color(0xFFA3A5B8),
    outlineVariant = Color(0xFF34364A),
)

private val AppTypography = Typography().run {
    copy(
        headlineMedium = headlineMedium.copy(fontWeight = FontWeight.SemiBold),
        titleLarge = titleLarge.copy(fontWeight = FontWeight.SemiBold),
        titleMedium = titleMedium.copy(fontWeight = FontWeight.SemiBold),
        labelLarge = TextStyle(fontWeight = FontWeight.SemiBold, fontSize = 14.sp, letterSpacing = 0.2.sp),
    )
}

@Composable
fun ExtendoTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (dark) Dark else Light,
        typography = AppTypography,
        shapes = Shapes(
            small = RoundedCornerShape(10.dp),
            medium = RoundedCornerShape(16.dp),
            large = RoundedCornerShape(24.dp),
        ),
        content = content,
    )
}

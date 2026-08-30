package io.github.dipakmdhrm.fodder.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext

@Composable
fun FodderTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val context = LocalContext.current
    val colors =
        when {
            // Material You where the platform offers it; the desktop app follows
            // the system accent the same way through libadwaita.
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ->
                if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
            darkTheme -> darkColorScheme()
            else -> lightColorScheme()
        }

    MaterialTheme(colorScheme = colors, content = content)
}

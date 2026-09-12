package io.github.dipakmdhrm.fodder.ui.components

import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable

/**
 * A confirmation for an irreversible action.
 *
 * The phone's version of the desktop viewer's `confirm_destructive` helper
 * (`fodder/src/app.rs`): the confirm button is tinted with the error color the
 * way libadwaita's `Destructive` response appearance is, and dismissing is
 * always the safe outcome.
 */
@Composable
fun ConfirmDialog(
    title: String,
    body: String,
    confirmLabel: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { Text(body) },
        confirmButton = {
            TextButton(
                onClick = onConfirm,
                colors =
                    ButtonDefaults.textButtonColors(
                        contentColor = MaterialTheme.colorScheme.error,
                    ),
            ) {
                Text(confirmLabel)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

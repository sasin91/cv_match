package io.github.sasin91.jobfinder.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.sasin91.jobfinder.PairState

/** Not paired yet: how to pair, and the two ways to (scan the QR code, or paste its link). */
@Composable
fun PairScreen(
    state: PairState,
    onScan: () -> Unit,
    onType: (String) -> Unit,
    onPair: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("JobFinder", style = MaterialTheme.typography.headlineLarge)
        Text(
            "See your shortlist and send job posts to JobFinder on your computer, wherever you are.",
            style = MaterialTheme.typography.bodyLarge,
        )
        PaperCard {
            Text("Pair with your computer", style = MaterialTheme.typography.titleMedium)
            Text(
                "1. Have Tailscale on, on this device and the computer, signed in to the same tailnet.\n" +
                    "2. On the computer, open JobFinder, then Settings, Devices, and press Show a pairing code.\n" +
                    "3. Scan the QR code. A code works once, for five minutes.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Button(onClick = onScan, enabled = !state.busy, modifier = Modifier.fillMaxWidth()) {
                Text("Scan the QR code")
            }
        }
        PaperCard {
            Text("Or paste the pairing link", style = MaterialTheme.typography.titleMedium)
            OutlinedTextField(
                value = state.linkText,
                onValueChange = onType,
                placeholder = { Text("jobfinder-pair://…") },
                singleLine = true,
                enabled = !state.busy,
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedButton(
                onClick = { onPair(state.linkText) },
                enabled = !state.busy && state.linkText.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
            ) { Text("Pair") }
        }
        if (state.busy) {
            CircularProgressIndicator()
            Text("Pairing…", style = MaterialTheme.typography.bodyMedium)
        }
        state.error?.let { ErrorCard(it) }
    }
}

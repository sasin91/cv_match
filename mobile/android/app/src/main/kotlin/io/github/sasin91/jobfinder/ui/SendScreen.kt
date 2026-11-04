package io.github.sasin91.jobfinder.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.sasin91.jobfinder.SendState
import io.github.sasin91.jobfinder.protocol.EVALUATION_DONE
import io.github.sasin91.jobfinder.protocol.EVALUATION_FAILED
import io.github.sasin91.jobfinder.protocol.EVALUATION_WORKING
import io.github.sasin91.jobfinder.protocol.Evaluation

/** The least height of the box a post is pasted in. */
private val POST_BOX_HEIGHT = 140.dp

/** Send a job post (a link or its text) to be matched, and see what became of the ones sent. */
@Composable
fun SendScreen(
    state: SendState,
    onType: (String) -> Unit,
    onSend: () -> Unit,
    onOpen: (Long) -> Unit,
    onUnpair: () -> Unit,
    modifier: Modifier = Modifier,
) {
    LazyColumn(
        modifier.fillMaxSize().padding(horizontal = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Column(Modifier.padding(top = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Match a job post", style = MaterialTheme.typography.titleLarge)
                Text(
                    "Paste a link or the post's text, or share it here from your browser. JobFinder on the computer " +
                        "matches it with your profile and saves it with your matches.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = state.text,
                    onValueChange = onType,
                    placeholder = { Text("https://… or the job post") },
                    enabled = !state.sending,
                    modifier = Modifier.fillMaxWidth().heightIn(min = POST_BOX_HEIGHT),
                )
                Button(
                    onClick = onSend,
                    enabled = !state.sending && state.text.isNotBlank(),
                    modifier = Modifier.fillMaxWidth(),
                ) { Text(if (state.sending) "Sending…" else "Match it") }
                state.error?.let { ErrorCard(it) }
            }
        }
        if (state.evaluations.isNotEmpty()) {
            item { Text("Sent from this device", style = MaterialTheme.typography.titleMedium) }
        }
        items(state.evaluations, key = { it.id }) { evaluation ->
            EvaluationItem(evaluation, onOpen)
        }
        item {
            TextButton(onClick = onUnpair, modifier = Modifier.padding(vertical = 16.dp)) {
                // Unpairing can't be undone from the phone: the danger colour, as the desktop's.
                Text("Unpair this device", color = MaterialTheme.colorScheme.error)
            }
        }
    }
}

@Composable
private fun EvaluationItem(evaluation: Evaluation, onOpen: (Long) -> Unit) {
    val open = evaluation.matchId?.takeIf { evaluation.state == EVALUATION_DONE }
    PaperCard(if (open != null) Modifier.clickable { onOpen(open) } else Modifier) {
        Text(evaluation.heading.ifEmpty { evaluation.sent }, style = MaterialTheme.typography.titleSmall)
        when (evaluation.state) {
            EVALUATION_WORKING -> LinearProgressIndicator(Modifier.fillMaxWidth())
            EVALUATION_DONE -> evaluation.percent?.let { percent ->
                Text("$percent% · ${evaluation.tagName}", color = Marks.of(evaluation.tag), style = MaterialTheme.typography.bodyMedium)
            }
        }
        Text(
            evaluation.said,
            style = MaterialTheme.typography.bodySmall,
            color = if (evaluation.state == EVALUATION_FAILED) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

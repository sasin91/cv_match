package io.github.sasin91.jobfinder.ui

import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import io.github.sasin91.jobfinder.MatchState
import io.github.sasin91.jobfinder.protocol.MatchDetails
import io.github.sasin91.jobfinder.protocol.Requirement
import io.github.sasin91.jobfinder.protocol.SavedTime

/** The size of the score ring. */
private val RING = 72.dp

/** How far an alternative is set in under the requirement it is one of. */
private val ALTERNATIVE_INDENT = 16.dp

/** A saved match: its score, the job, whether it is open, and each requirement with its verdict. */
@Composable
fun MatchScreen(state: MatchState, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    val details = state.details
    Column(
        modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        when {
            details != null -> Details(details)
            state.loading -> CircularProgressIndicator()
            state.error != null -> {
                ErrorCard(state.error)
                Button(onClick = onRetry) { Text("Try again") }
            }
        }
    }
}

@Composable
private fun Details(details: MatchDetails) {
    val context = LocalContext.current
    Row(verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(RING), contentAlignment = Alignment.Center) {
            CircularProgressIndicator(
                progress = { details.percent / 100f },
                modifier = Modifier.size(RING),
                color = Marks.of(details.tag),
                trackColor = MaterialTheme.colorScheme.surfaceVariant,
            )
            Text("${details.percent}%", style = MaterialTheme.typography.titleMedium)
        }
        Spacer(Modifier.width(16.dp))
        Column {
            Text(details.heading, style = MaterialTheme.typography.titleLarge)
            Text(details.tagName, color = Marks.of(details.tag), style = MaterialTheme.typography.bodyMedium)
        }
    }
    if (details.parts.isNotEmpty()) {
        Text(details.parts, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
    details.opening?.let { opening ->
        Text(
            opening.text,
            style = MaterialTheme.typography.bodyMedium,
            color = if (opening.closed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
        )
    }
    if (details.jobUrl.isNotEmpty()) {
        Button(
            onClick = { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(details.jobUrl))) },
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Open job post") }
    }
    if (details.disqualifiers.isNotEmpty()) {
        PaperCard {
            Text("Likely rules it out", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.error)
            details.disqualifiers.forEach { RequirementItem(it) }
        }
    }
    details.sections.filter { it.rows.isNotEmpty() }.forEach { section ->
        PaperCard {
            Text(section.heading, style = MaterialTheme.typography.titleMedium)
            if (section.note.isNotEmpty()) {
                Text(section.note, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            section.rows.forEach { RequirementItem(it) }
        }
    }
    if (details.savedAt.isNotEmpty()) {
        Text("Saved ${SavedTime.of(details.savedAt)}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun RequirementItem(requirement: Requirement, indent: Boolean = false) {
    Column(
        Modifier.padding(start = if (indent) ALTERNATIVE_INDENT else 0.dp, top = 4.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Row {
            Text(verdictLabel(requirement.verdict), color = Marks.of(requirement.verdict), style = MaterialTheme.typography.labelLarge)
            Spacer(Modifier.width(8.dp))
            Text(requirement.text, style = MaterialTheme.typography.bodyMedium)
        }
        if (requirement.reason.isNotEmpty()) {
            Text(requirement.reason, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (requirement.evidence.isNotEmpty()) {
            Text("“${requirement.evidence}”", style = MaterialTheme.typography.bodySmall)
        }
        requirement.alternatives.forEach { RequirementItem(it, indent = true) }
    }
}

private fun verdictLabel(verdict: String) = when (verdict) {
    "met" -> "Met"
    "partial" -> "Partial"
    else -> "Missing"
}

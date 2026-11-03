package io.github.sasin91.jobfinder.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import io.github.sasin91.jobfinder.ListState
import io.github.sasin91.jobfinder.protocol.MatchRow

/** A list of saved matches, best first: the shortlist, or another list chosen above it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MatchesScreen(
    state: ListState,
    onList: (String) -> Unit,
    onRefresh: () -> Unit,
    onMore: () -> Unit,
    onOpen: (Long) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier.fillMaxSize()) {
        if (state.lists.isNotEmpty()) {
            Row(
                Modifier
                    .horizontalScroll(rememberScrollState())
                    .padding(horizontal = 12.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                state.lists.forEach { list ->
                    FilterChip(
                        selected = list.name == state.list,
                        onClick = { onList(list.name) },
                        label = { Text(list.label) },
                    )
                }
            }
        }
        PullToRefreshBox(isRefreshing = state.loading, onRefresh = onRefresh, modifier = Modifier.weight(1f)) {
            LazyColumn(Modifier.fillMaxSize()) {
                state.error?.let { error -> item { ErrorCard(error, Modifier.padding(12.dp)) } }
                if (state.loaded && state.rows.isEmpty() && state.error == null) {
                    item {
                        val empty = state.lists.firstOrNull { it.name == state.list }?.empty ?: "No saved matches yet."
                        Text(
                            empty,
                            modifier = Modifier.padding(24.dp),
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                items(state.rows, key = { it.id }) { row ->
                    MatchRowItem(row, onClick = { onOpen(row.id) })
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                }
                if (state.nextAfter != null) {
                    item {
                        TextButton(onClick = onMore, enabled = !state.loading, modifier = Modifier.fillMaxWidth()) {
                            Text("Show more")
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun MatchRowItem(row: MatchRow, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
    ) {
        Text("●", color = Marks.of(row.tag), style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.width(12.dp))
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(row.title, style = MaterialTheme.typography.titleMedium)
            Text(row.about, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (row.opening.isNotEmpty()) {
                Text(
                    row.opening,
                    style = MaterialTheme.typography.bodySmall,
                    color = if (row.closed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            val saved = listOf(row.savedAt, row.done).filter { it.isNotEmpty() }.joinToString(" · ")
            if (saved.isNotEmpty()) {
                Text(saved, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

package io.github.sasin91.jobfinder.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import io.github.sasin91.jobfinder.AppModel
import io.github.sasin91.jobfinder.Screen

/** The whole app: pairing until paired, then the lists, a match, and sending posts. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun JobFinderApp(model: AppModel) {
    val pairing by model.pairing.collectAsState()
    val screen by model.screen.collectAsState()
    val pair by model.pair.collectAsState()

    BackHandler(enabled = screen != Screen.Matches) { model.back() }

    if (screen == Screen.Scan) {
        ScanScreen(onFound = model::pairWith, onCancel = { model.back() })
        return
    }
    if (pairing == null) {
        Scaffold(containerColor = MaterialTheme.colorScheme.background) { padding ->
            PairScreen(
                state = pair,
                onScan = { model.show(Screen.Scan) },
                onType = model::typeLink,
                onPair = model::pairWith,
                modifier = Modifier.padding(padding),
            )
        }
        return
    }

    LaunchedEffect(pairing) { model.loadFirst() }
    val list by model.list.collectAsState()
    val match by model.match.collectAsState()
    val send by model.send.collectAsState()

    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        topBar = {
            TopAppBar(
                title = { Text(if (screen is Screen.Match) "Match" else "JobFinder") },
                navigationIcon = {
                    if (screen is Screen.Match) TextButton(onClick = { model.back() }) { Text("Back") }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        bottomBar = {
            NavigationBar(containerColor = MaterialTheme.colorScheme.surface) {
                NavigationBarItem(
                    selected = screen == Screen.Matches || screen is Screen.Match,
                    onClick = { model.show(Screen.Matches) },
                    icon = { Text("☰") },
                    label = { Text("Matches") },
                )
                NavigationBarItem(
                    selected = screen == Screen.Send,
                    onClick = { model.show(Screen.Send) },
                    icon = { Text("➤") },
                    label = { Text("Send a post") },
                )
            }
        },
    ) { padding ->
        val inside = Modifier.padding(padding)
        when (val shown = screen) {
            Screen.Matches -> MatchesScreen(
                state = list,
                onList = model::chooseList,
                onRefresh = model::refresh,
                onMore = model::loadMore,
                onOpen = { id -> model.show(Screen.Match(id)) },
                modifier = inside,
            )
            is Screen.Match -> MatchScreen(state = match, onRetry = { model.loadMatch(shown.id) }, modifier = inside)
            Screen.Send -> SendScreen(
                state = send,
                onType = model::typePost,
                onSend = model::sendPost,
                onOpen = { id -> model.show(Screen.Match(id)) },
                onUnpair = model::unpair,
                modifier = inside,
            )
            Screen.Scan -> Unit
        }
    }
}

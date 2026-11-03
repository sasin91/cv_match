package io.github.sasin91.jobfinder.ui

import android.annotation.SuppressLint
import android.graphics.Bitmap
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import io.github.sasin91.jobfinder.protocol.protocolJson
import kotlinx.serialization.builtins.serializer

/** What reads the page as the web view shows it: its whole HTML, as a JSON string. */
private const val PAGE_HTML_SCRIPT = "document.documentElement.outerHTML"

/**
 * A shared or pasted link, opened in the app. The page shows as in a browser, so a site's check
 * (Indeed's Cloudflare) is passed here, by you; Match it then sends the page as shown to the
 * computer, which reads it as the Jobnet extension's pages are read and never fetches it itself.
 */
@SuppressLint("SetJavaScriptEnabled")
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReaderScreen(
    url: String,
    onMatch: (url: String, html: String) -> Unit,
    onSendLink: (String) -> Unit,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    var loading by remember { mutableStateOf(true) }
    var shown by remember { mutableStateOf(false) }
    var reading by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val webView = remember {
        WebView(context).apply {
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            webViewClient = object : WebViewClient() {
                override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
                    loading = true
                }

                override fun onPageFinished(view: WebView, url: String) {
                    loading = false
                    shown = true
                }
            }
            loadUrl(url)
        }
    }
    DisposableEffect(webView) { onDispose { webView.destroy() } }

    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        topBar = {
            TopAppBar(
                title = { Text("Job post") },
                navigationIcon = { TextButton(onClick = onBack) { Text("Back") } },
                actions = {
                    Button(
                        onClick = {
                            reading = true
                            error = null
                            webView.evaluateJavascript(PAGE_HTML_SCRIPT) { result ->
                                reading = false
                                val html = runCatching { protocolJson.decodeFromString(String.serializer(), result) }
                                    .getOrNull()
                                if (html.isNullOrBlank()) {
                                    error = "The page couldn't be read. Send its link instead."
                                } else {
                                    onMatch(webView.url ?: url, html)
                                }
                            }
                        },
                        enabled = shown && !reading,
                        modifier = Modifier.padding(end = 8.dp),
                    ) { Text("Match it") }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (loading) LinearProgressIndicator(Modifier.fillMaxWidth())
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "Pass any check the site asks for, then tap Match it.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = { onSendLink(webView.url ?: url) }) { Text("Send the link") }
            }
            error?.let { ErrorCard(it, Modifier.padding(horizontal = 16.dp)) }
            AndroidView(factory = { webView }, modifier = Modifier.fillMaxSize())
        }
    }
}

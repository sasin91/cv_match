package io.github.sasin91.jobfinder

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import io.github.sasin91.jobfinder.protocol.PAIRING_SCHEME
import io.github.sasin91.jobfinder.ui.JobFinderApp
import io.github.sasin91.jobfinder.ui.JobFinderTheme

class MainActivity : ComponentActivity() {
    private val model: AppModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (savedInstanceState == null) opened(intent)
        setContent { JobFinderTheme { JobFinderApp(model) } }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        opened(intent)
    }

    /** A pairing link the camera app scanned, or a post shared from another app. */
    private fun opened(intent: Intent?) {
        when (intent?.action) {
            Intent.ACTION_VIEW -> intent.data?.takeIf { it.scheme == PAIRING_SCHEME }?.let { model.pairWith(it.toString()) }
            Intent.ACTION_SEND -> {
                val text = listOfNotNull(
                    intent.getStringExtra(Intent.EXTRA_TEXT),
                ).joinToString("\n").trim()
                if (text.isNotEmpty()) model.shared(text)
            }
        }
    }
}

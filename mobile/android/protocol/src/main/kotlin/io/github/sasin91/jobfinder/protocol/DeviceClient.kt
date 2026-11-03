package io.github.sasin91.jobfinder.protocol

import java.io.IOException
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

/** How long the computer may take to answer, in seconds: a list or a match is read in well under. */
private const val ANSWER_SECONDS = 30L

/** How long reaching the computer may take, in seconds, over Tailscale from anywhere. */
private const val CONNECT_SECONDS = 15L

/** The HTTP status the computer answers when a token isn't a paired device's. */
private const val NOT_PAIRED_STATUS = 401

private val JSON_TYPE = "application/json".toMediaType()

/** Reads what the computer sends, leaving out what this version doesn't know. */
val protocolJson = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
}

/**
 * Why a request failed, in words to show. notPaired: the computer doesn't know this device's
 * token (it was removed there), so the app pairs again.
 */
class DeviceError(message: String, val notPaired: Boolean = false) : Exception(message)

/**
 * Talks to JobFinder on the computer at address (https://pc.tailnet.ts.net:8766). Tailscale's
 * certificate is a real one, so the platform's own TLS checks it. token: this device's, once paired.
 */
class DeviceClient(
    private val address: String,
    private val token: String? = null,
    private val http: OkHttpClient = defaultHttp,
) {
    suspend fun pair(link: PairingLink, name: String): PairAnswer =
        post(PAIR_PATH, PairAsk(link.code, name), PairAsk.serializer(), PairAnswer.serializer())

    suspend fun hello(): Hello = post(HELLO_PATH, null, null, Hello.serializer())

    suspend fun matches(list: String?, after: String = ""): Matches =
        post(MATCHES_PATH, MatchesAsk(list, after), MatchesAsk.serializer(), Matches.serializer())

    suspend fun match(id: Long): MatchDetails =
        post(MATCH_PATH, MatchAsk(id), MatchAsk.serializer(), MatchDetails.serializer())

    suspend fun evaluate(text: String): Evaluation =
        post(EVALUATE_PATH, EvaluateAsk(text), EvaluateAsk.serializer(), Evaluation.serializer())

    suspend fun evaluations(): Evaluations = post(EVALUATIONS_PATH, null, null, Evaluations.serializer())

    private suspend fun <A, T> post(
        path: String,
        ask: A?,
        askSerializer: KSerializer<A>?,
        answerSerializer: KSerializer<T>,
    ): T = withContext(Dispatchers.IO) {
        val body = if (ask != null && askSerializer != null) protocolJson.encodeToString(askSerializer, ask) else "{}"
        val request = Request.Builder()
            .url(address.trimEnd('/') + path)
            .post(body.toRequestBody(JSON_TYPE))
            .apply { token?.let { header("Authorization", "Bearer $it") } }
            .build()
        val (status, text) = try {
            http.newCall(request).execute().use { response -> response.code to (response.body?.string() ?: "") }
        } catch (e: IOException) {
            throw DeviceError(
                "Couldn't reach JobFinder at $address. Check that Tailscale is on, on this device and the computer, " +
                    "and that JobFinder is running there. (${e.message ?: e.javaClass.simpleName})",
            )
        }
        if (status !in 200..299) {
            val why = runCatching { protocolJson.decodeFromString(Refusal.serializer(), text).error }
                .getOrNull()
                ?.takeIf { it.isNotBlank() }
                ?: "JobFinder on the computer answered $status."
            throw DeviceError(why, notPaired = status == NOT_PAIRED_STATUS)
        }
        try {
            protocolJson.decodeFromString(answerSerializer, text)
        } catch (e: IllegalArgumentException) {
            throw DeviceError("JobFinder on the computer answered something this app can't read: update both. (${e.message})")
        }
    }

    companion object {
        val defaultHttp: OkHttpClient = OkHttpClient.Builder()
            .connectTimeout(CONNECT_SECONDS, TimeUnit.SECONDS)
            .readTimeout(ANSWER_SECONDS, TimeUnit.SECONDS)
            .build()
    }
}

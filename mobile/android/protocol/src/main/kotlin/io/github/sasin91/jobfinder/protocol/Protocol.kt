// What JobFinder on the computer and this app send each other. It mirrors app/device-protocol in
// the JobFinder repo, which the computer's side is built from: change both together.
package io.github.sasin91.jobfinder.protocol

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** The port JobFinder on the computer answers devices on, through `tailscale serve`. */
const val PORT = 8766

/** Where a device trades its pairing code for a token. */
const val PAIR_PATH = "/v1/device/pair"

/** Where a paired device asks what it is paired with. */
const val HELLO_PATH = "/v1/device/hello"

/** Where a paired device reads a list of saved matches, best first. */
const val MATCHES_PATH = "/v1/device/matches"

/** Where a paired device reads one saved match. */
const val MATCH_PATH = "/v1/device/match"

/** Where a paired device sends a job post, a link or its text, to be matched. */
const val EVALUATE_PATH = "/v1/device/evaluate"

/** Where a paired device reads what became of the job posts it sent. */
const val EVALUATIONS_PATH = "/v1/device/evaluations"

/** Most characters of a job post the computer takes at EVALUATE_PATH. */
const val MAX_EVALUATE_CHARS = 100_000

/** The list shown first. */
const val SHORTLIST = "shortlist"

/** Evaluation.state while the post is being read and matched. */
const val EVALUATION_WORKING = "working"

/** Evaluation.state once it is matched. */
const val EVALUATION_DONE = "done"

/** Evaluation.state when it couldn't be matched; Evaluation.said says why. */
const val EVALUATION_FAILED = "failed"

@Serializable
data class PairAsk(val code: String, val name: String)

@Serializable
data class PairAnswer(val token: String)

@Serializable
data class Hello(
    val version: String,
    @SerialName("device_name") val deviceName: String,
)

/** What the computer answers when it refuses or fails: why, in words to show. */
@Serializable
data class Refusal(val error: String)

@Serializable
data class MatchesAsk(val list: String? = null, val after: String = "")

@Serializable
data class ListName(val name: String, val label: String, val empty: String)

@Serializable
data class Matches(
    val lists: List<ListName>,
    val list: String,
    val matches: List<MatchRow>,
    @SerialName("next_after") val nextAfter: String? = null,
)

@Serializable
data class MatchRow(
    val id: Long,
    /** "top", "good", "fair" or "low". */
    val tag: String,
    val title: String,
    /** "81% good · likely disqualifier: Located close to Copenhagen". */
    val about: String,
    @SerialName("saved_at") val savedAt: String,
    @SerialName("saved_on") val savedOn: String,
    val done: String,
    val opening: String,
    val closed: Boolean,
)

@Serializable
data class MatchAsk(val id: Long)

@Serializable
data class MatchDetails(
    val id: Long,
    val percent: Long,
    val tag: String,
    @SerialName("tag_name") val tagName: String,
    val heading: String,
    val parts: String,
    @SerialName("saved_at") val savedAt: String,
    @SerialName("job_url") val jobUrl: String,
    val opening: OpeningText? = null,
    val disqualifiers: List<Requirement> = emptyList(),
    val sections: List<RequirementGroup> = emptyList(),
)

@Serializable
data class OpeningText(val text: String, val closed: Boolean)

@Serializable
data class RequirementGroup(val heading: String, val note: String, val rows: List<Requirement>)

@Serializable
data class Requirement(
    /** "missing", "partial" or "met". */
    val verdict: String,
    val text: String,
    val reason: String,
    val evidence: String,
    @SerialName("decided_by") val decidedBy: String,
    val alternatives: List<Requirement> = emptyList(),
)

@Serializable
data class EvaluateAsk(val text: String)

@Serializable
data class Evaluation(
    val id: Long,
    val sent: String,
    @SerialName("sent_at") val sentAt: Long,
    val state: String,
    val said: String,
    @SerialName("match_id") val matchId: Long? = null,
    val percent: Long? = null,
    val tag: String = "",
    @SerialName("tag_name") val tagName: String = "",
    val heading: String = "",
)

@Serializable
data class Evaluations(val evaluations: List<Evaluation>)

package io.github.sasin91.jobfinder.protocol

/** The scheme of a pairing link: jobfinder-pair://<host>:<port>/<code>. */
const val PAIRING_SCHEME = "jobfinder-pair"

/** The ports there are: a link with another one is no pairing link. */
private val PORTS = 1..65535

/**
 * What the QR code on the computer holds: where to reach it, and the code that pairs this
 * device once. Read the same way as the computer's PairingLink::parse.
 */
data class PairingLink(val host: String, val port: Int, val code: String) {
    /** Where requests go: https://pc.tailnet.ts.net:8766. */
    val address: String get() = "https://$host:$port"

    companion object {
        /** The pairing link in text, or null when it isn't one. */
        fun parse(text: String): PairingLink? {
            val rest = text.trim().removePrefix("$PAIRING_SCHEME://")
            if (rest == text.trim()) return null
            val slash = rest.indexOf('/')
            if (slash < 0) return null
            val address = rest.substring(0, slash)
            val code = rest.substring(slash + 1)
            val colon = address.lastIndexOf(':')
            if (colon < 0) return null
            val host = address.substring(0, colon)
            val port = address.substring(colon + 1).toIntOrNull()?.takeIf { it in PORTS } ?: return null
            if (!isHostName(host) || !isCode(code)) return null
            return PairingLink(host, port, code)
        }

        private fun isHostName(host: String) =
            host.isNotEmpty() && host.all { it.isLetterOrDigit() && it.code < 128 || it == '-' || it == '.' }

        private fun isCode(code: String) = code.isNotEmpty() && code.all { it in '0'..'9' || it in 'a'..'f' }
    }
}

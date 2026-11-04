package io.github.sasin91.jobfinder.protocol

import java.time.Instant
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeParseException
import java.util.Locale

/** When something was saved, as a person says it in this device's time zone. */
object SavedTime {
    private val CLOCK = DateTimeFormatter.ofPattern("HH:mm", Locale.ENGLISH)
    private val DAY = DateTimeFormatter.ofPattern("d MMM", Locale.ENGLISH)
    private val DAY_AND_YEAR = DateTimeFormatter.ofPattern("d MMM yyyy", Locale.ENGLISH)

    /**
     * The time the computer wrote as iso ("2026-11-03T14:34:41Z"): "today 15:34",
     * "yesterday 09:12", "3 Nov", or "3 Nov 2025" in another year. What can't be read is shown
     * as it came.
     */
    fun of(iso: String, now: ZonedDateTime = ZonedDateTime.now()): String {
        val at = try {
            Instant.parse(iso.trim()).atZone(now.zone)
        } catch (e: DateTimeParseException) {
            return iso
        }
        val day = at.toLocalDate()
        val today = now.toLocalDate()
        return when {
            day == today -> "today ${at.format(CLOCK)}"
            day == today.minusDays(1) -> "yesterday ${at.format(CLOCK)}"
            day.year == today.year -> at.format(DAY)
            else -> at.format(DAY_AND_YEAR)
        }
    }
}

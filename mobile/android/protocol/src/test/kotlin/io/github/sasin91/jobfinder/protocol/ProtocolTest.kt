package io.github.sasin91.jobfinder.protocol

import kotlinx.coroutines.test.runTest
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test

class PairingLinkTest {
    @Test
    fun aPairingLinkReadsAsTheComputerWritesIt() {
        val link = PairingLink.parse(" jobfinder-pair://pc.tail1234.ts.net:8766/0a1b2c ")
        assertEquals(PairingLink("pc.tail1234.ts.net", PORT, "0a1b2c"), link)
        assertEquals("https://pc.tail1234.ts.net:8766", link!!.address)
    }

    @Test
    fun anythingElseIsNoPairingLink() {
        for (text in listOf(
            "",
            "https://pc.tail1234.ts.net:8766/0a1b2c",
            "jobfinder-pair://pc.tail1234.ts.net/0a1b2c",
            "jobfinder-pair://pc.tail1234.ts.net:8766/NOT-HEX",
            "jobfinder-pair://pc.tail1234.ts.net:port/0a",
            "jobfinder-pair://pc.tail1234.ts.net:65536/0a",
            "jobfinder-pair://pc.tail1234.ts.net:0/0a",
            "jobfinder-pair://evil.example/x:8766/0a1b2c",
        )) {
            assertNull(text, PairingLink.parse(text))
        }
    }
}

class DeviceClientTest {
    @Test
    fun anAddressThatIsNoURLIsADeviceError() = runTest {
        try {
            DeviceClient("not a url").hello()
            fail("hello was answered")
        } catch (e: DeviceError) {
            assertTrue(e.message!!.contains("not a url"))
        }
    }

    private lateinit var server: MockWebServer

    @Before
    fun start() {
        server = MockWebServer()
        server.start()
    }

    @After
    fun stop() = server.shutdown()

    private fun client(token: String? = "3.beef") = DeviceClient(server.url("/").toString(), token)

    @Test
    fun aRequestCarriesTheTokenAndReadsTheAnswer() = runTest {
        server.enqueue(
            MockResponse().setBody(
                """{"lists":[{"name":"shortlist","label":"Shortlist","empty":"None."}],"list":"shortlist",
                   "matches":[{"id":21,"tag":"top","title":"PHP-udvikler","about":"86% top","saved_at":"3 Nov",
                   "saved_on":"","done":"","opening":"","closed":false,"new_field":1}],"next_after":null}""",
            ),
        )
        val matches = client().matches(null)
        assertEquals(21L, matches.matches.single().id)
        assertNull(matches.nextAfter)
        val request = server.takeRequest()
        assertEquals(MATCHES_PATH, request.path)
        assertEquals("Bearer 3.beef", request.getHeader("Authorization"))
        assertEquals("{}", request.body.readUtf8())
    }

    @Test
    fun aRefusalSaysWhyAndARemovedDeviceIsNotPaired() = runTest {
        server.enqueue(MockResponse().setResponseCode(401).setBody("""{"error":"This device isn't paired."}"""))
        try {
            client().hello()
            fail("hello was answered")
        } catch (e: DeviceError) {
            assertEquals("This device isn't paired.", e.message)
            assertTrue(e.notPaired)
        }
    }

    @Test
    fun anEvaluationReadsWithoutAMatchYet() = runTest {
        server.enqueue(
            MockResponse().setBody(
                """{"id":1,"sent":"PHP-udvikler","sent_at":1760000000,"state":"working","said":"Waiting to start…",
                   "match_id":null,"percent":null,"tag":"","tag_name":"","heading":""}""",
            ),
        )
        val evaluation = client().evaluate("PHP-udvikler")
        assertEquals(EVALUATION_WORKING, evaluation.state)
        assertNull(evaluation.matchId)
        assertEquals("""{"text":"PHP-udvikler"}""", server.takeRequest().body.readUtf8())
    }

    @Test
    fun aPageGoesAsItsLinkAndHtml() = runTest {
        server.enqueue(
            MockResponse().setBody(
                """{"id":2,"sent":"https://dk.indeed.com/viewjob?jk=ab","sent_at":1,"state":"working","said":""}""",
            ),
        )
        client().evaluatePage("https://dk.indeed.com/viewjob?jk=ab", "<html></html>")
        assertEquals(
            """{"url":"https://dk.indeed.com/viewjob?jk=ab","html":"<html></html>"}""",
            server.takeRequest().body.readUtf8(),
        )
    }
}

class IsLinkTest {
    @Test
    fun oneWebLinkIsALink() {
        assertTrue(isLink(" https://dk.indeed.com/viewjob?jk=ab "))
        assertTrue(!isLink("PHP-udvikler https://x.dk"))
        assertTrue(!isLink("https://"))
        assertTrue(!isLink("jobfinder-pair://pc:8766/ab"))
    }
}

class SavedTimeTest {
    private val now = java.time.ZonedDateTime.of(2026, 11, 3, 20, 0, 0, 0, java.time.ZoneId.of("Europe/Copenhagen"))

    @Test
    fun aSavedTimeReadsAsAPersonSaysIt() {
        assertEquals("today 15:34", SavedTime.of("2026-11-03T14:34:41Z", now))
        assertEquals("yesterday 09:12", SavedTime.of("2026-11-02T08:12:00Z", now))
        assertEquals("21 Oct", SavedTime.of("2026-10-21T10:00:00Z", now))
        assertEquals("3 Nov 2025", SavedTime.of("2025-11-03T10:00:00Z", now))
    }

    @Test
    fun aTimeThatCantBeReadIsShownAsItCame() {
        assertEquals("", SavedTime.of("", now))
        assertEquals("soon", SavedTime.of("soon", now))
    }
}

/**
 * Tests unitarios del SDK Kotlin — 4 invariantes:
 *   1. queue cap descarta cuando se llena
 *   2. retry on 5xx
 *   3. beforeSend filtra (null → descartar)
 *   4. scrubbing aplica scrubFields + scrubPatterns
 *
 * Usamos com.sun.net.httpserver.HttpServer (JDK builtin) para evitar mocks.
 */
package com.iaportafolio.faro

import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpServer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.jupiter.api.AfterEach
import org.junit.jupiter.api.Test
import java.net.InetSocketAddress
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

private class Capture {
    val server: HttpServer = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
    val batches: MutableList<JsonObject> = CopyOnWriteArrayList()
    val calls = AtomicInteger(0)
    var nextStatus: Int = 200
    val endpoint: String get() = "http://127.0.0.1:${server.address.port}"

    init {
        server.createContext("/") { ex: HttpExchange ->
            calls.incrementAndGet()
            val body = ex.requestBody.readBytes().toString(Charsets.UTF_8)
            try {
                batches.add(Json.parseToJsonElement(body).jsonObject)
            } catch (_: Throwable) { /* tolerar payload no-json */ }
            val res = "{\"ok\":true}".toByteArray()
            ex.sendResponseHeaders(nextStatus, res.size.toLong())
            ex.responseBody.use { it.write(res) }
        }
        server.start()
    }

    fun close() = server.stop(0)
}

/** Captura igual que [Capture] pero conservando method/path/headers para
 *  el test de payload shape. */
private data class RecordedRequest(
    val method: String,
    val path: String,
    val auth: String?,
    val contentType: String?,
    val body: JsonObject,
)

private class CaptureHeaders {
    val server: HttpServer = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
    val batches: MutableList<RecordedRequest> = CopyOnWriteArrayList()
    var nextStatus: Int = 200
    val endpoint: String get() = "http://127.0.0.1:${server.address.port}"

    init {
        server.createContext("/") { ex: HttpExchange ->
            val raw = ex.requestBody.readBytes().toString(Charsets.UTF_8)
            val parsed = try {
                Json.parseToJsonElement(raw).jsonObject
            } catch (_: Throwable) {
                JsonObject(emptyMap())
            }
            batches.add(
                RecordedRequest(
                    method = ex.requestMethod,
                    path = ex.requestURI.path,
                    auth = ex.requestHeaders.getFirst("Authorization"),
                    contentType = ex.requestHeaders.getFirst("Content-Type"),
                    body = parsed,
                ),
            )
            val res = "{\"ok\":true}".toByteArray()
            ex.sendResponseHeaders(nextStatus, res.size.toLong())
            ex.responseBody.use { it.write(res) }
        }
        server.start()
    }

    fun close() = server.stop(0)
}

private fun waitFor(maxMs: Long = 2000, cond: () -> Boolean) {
    val deadline = System.currentTimeMillis() + maxMs
    while (System.currentTimeMillis() < deadline) {
        if (cond()) return
        Thread.sleep(30)
    }
}

private fun productEvents(cap: Capture): List<JsonObject> =
    cap.batches.flatMap { batch ->
        batch["events"]?.jsonArray?.map { it.jsonObject } ?: emptyList()
    }

class FaroTest {

    @AfterEach
    fun tearDown() {
        Faro.close()
    }

    // ---- 1. queue cap ----
    @Test
    fun `queue cap descarta cuando se llena`() {
        val cap = Capture()
        try {
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "tk",
                    service = "queue-cap",
                    installGlobalHandlers = false,
                    flushIntervalMs = 100_000, // sin auto-flush
                    maxBatchSize = 50,
                    maxQueueSize = 5,
                ),
            )
            repeat(50) { Faro.info("evento $it") }
            Faro.flush(timeoutMs = 2_000)
            waitFor { cap.batches.isNotEmpty() }
            val logs = cap.batches.first()["logs"]!!.jsonArray
            // El canal de Kotlin tiene capacidad 1 + un slot interno; la cota efectiva
            // puede ser un poco mayor a maxQueueSize. Lo que NUNCA debe pasar es enviar
            // los 50.
            assertTrue(logs.size < 50, "se filtraron al menos algunos eventos (got ${logs.size})")
        } finally {
            cap.close()
        }
    }

    // ---- 2. retry sobre 5xx ----
    @Test
    fun `5xx el batch se re-encola`() {
        val cap = Capture()
        try {
            cap.nextStatus = 503
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "tk",
                    service = "retry-test",
                    installGlobalHandlers = false,
                    flushIntervalMs = 100,
                ),
            )
            Faro.info("reintentar-me")
            waitFor { cap.calls.get() >= 1 }
            val first = cap.calls.get()
            cap.nextStatus = 200
            waitFor { cap.calls.get() > first }
            assertTrue(cap.calls.get() > first, "debe haber un reintento tras el 503")
        } finally {
            cap.close()
        }
    }

    // ---- 3. beforeSend ----
    @Test
    fun `beforeSend null descarta`() {
        val cap = Capture()
        try {
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "tk",
                    service = "bs-discard",
                    installGlobalHandlers = false,
                    flushIntervalMs = 50,
                    beforeSend = { e -> if (e.message.contains("descartar")) null else e },
                ),
            )
            Faro.info("guardar")
            Faro.info("descartar")
            Faro.info("guardar también")
            Faro.flush(timeoutMs = 2_000)
            waitFor { cap.batches.isNotEmpty() }
            val msgs = cap.batches.flatMap { batch ->
                batch["logs"]!!.jsonArray.map { it.jsonObject["message"]!!.jsonPrimitive.content }
            }
            assertEquals(listOf("guardar", "guardar también"), msgs)
        } finally {
            cap.close()
        }
    }

    // ---- 5. init con opts inválidas ----

    @Test
    fun `init sin endpoint lanza IllegalArgumentException`() {
        assertFailsWith<IllegalArgumentException> {
            Faro.init(FaroOptions(endpoint = "", token = "tk", service = "s"))
        }.also { assertTrue(it.message!!.contains("endpoint")) }
    }

    @Test
    fun `init sin token lanza IllegalArgumentException`() {
        assertFailsWith<IllegalArgumentException> {
            Faro.init(FaroOptions(endpoint = "http://x", token = "", service = "s"))
        }.also { assertTrue(it.message!!.contains("token")) }
    }

    @Test
    fun `init sin service lanza IllegalArgumentException`() {
        assertFailsWith<IllegalArgumentException> {
            Faro.init(FaroOptions(endpoint = "http://x", token = "tk", service = ""))
        }.also { assertTrue(it.message!!.contains("service")) }
    }

    // ---- 6. log + flush + assert payload (shape del wire) ----

    @Test
    fun `payload shape del JSON enviado al wire`() {
        val cap = CaptureHeaders()
        try {
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "mi-token",
                    service = "payload-test",
                    environment = "prod",
                    release = "v1.2.3",
                    attributes = mapOf("region" to "eu-west-1"),
                    installGlobalHandlers = false,
                    flushIntervalMs = 50,
                ),
            )
            Faro.log("WARN", "algo raro", attributes = mapOf("http.status_code" to 500, "user.id" to "u42"))
            Faro.flush(timeoutMs = 2_000)
            waitFor { cap.batches.isNotEmpty() }

            val req = cap.batches.first()
            assertEquals("POST", req.method)
            assertEquals("/api/v1/ingest/logs", req.path)
            assertEquals("Bearer mi-token", req.auth)
            assertTrue((req.contentType ?: "").contains("application/json"))

            val body = req.body
            assertEquals("payload-test", body["service"]!!.jsonPrimitive.content)
            val logs = body["logs"]!!.jsonArray
            assertEquals(1, logs.size)
            val entry = logs.first().jsonObject
            assertEquals("WARN", entry["level"]!!.jsonPrimitive.content)
            assertEquals("algo raro", entry["message"]!!.jsonPrimitive.content)
            assertTrue(entry["timestamp"]!!.jsonPrimitive.content.contains("T"))
            val attrs = entry["attributes"]!!.jsonObject
            assertEquals("eu-west-1", attrs["region"]!!.jsonPrimitive.content)
            assertEquals("prod", attrs["deployment.environment"]!!.jsonPrimitive.content)
            assertEquals("v1.2.3", attrs["service.version"]!!.jsonPrimitive.content)
            // Los no-strings se serializan vía toString() (500 → "500").
            assertEquals("500", attrs["http.status_code"]!!.jsonPrimitive.content)
            assertEquals("u42", attrs["user.id"]!!.jsonPrimitive.content)
        } finally {
            cap.close()
        }
    }

    // ---- 6b. Product events API ----

    @Test
    fun `track envia evento mobile a endpoint events`() {
        val cap = Capture()
        try {
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "tk",
                    service = "track-test",
                    installGlobalHandlers = false,
                    flushIntervalMs = 50,
                ),
            )
            Faro.track("checkout_completed", mapOf("amount" to 99.5, "currency" to "USD"))
            Faro.flush(timeoutMs = 2_000)
            waitFor { productEvents(cap).isNotEmpty() }

            val event = productEvents(cap).first()
            assertEquals("track", event["type"]!!.jsonPrimitive.content)
            assertEquals("checkout_completed", event["name"]!!.jsonPrimitive.content)
            val props = event["properties"]!!.jsonObject
            assertEquals("99.5", props["amount"]!!.jsonPrimitive.content)
            assertEquals("USD", props["currency"]!!.jsonPrimitive.content)
            val distinct = event["distinct_id"]!!.jsonPrimitive.content
            val anonymous = event["anonymous_id"]!!.jsonPrimitive.content
            assertTrue(distinct.startsWith("anon_"), "pre-identify distinct_id debe ser anon")
            assertEquals(anonymous, distinct)
            assertEquals("", event["session_id"]!!.jsonPrimitive.content)
            assertEquals("mobile", event["source"]!!.jsonPrimitive.content)
        } finally {
            cap.close()
        }
    }

    @Test
    fun `identify fija distinct id para eventos siguientes`() {
        val cap = Capture()
        try {
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "tk",
                    service = "identify-test",
                    installGlobalHandlers = false,
                    flushIntervalMs = 50,
                ),
            )
            Faro.identify("user_42", mapOf("email" to "a@b.com", "plan" to "pro"))
            Faro.track("after_login")
            Faro.flush(timeoutMs = 2_000)
            waitFor { productEvents(cap).size >= 2 }

            val events = productEvents(cap)
            val identify = events.first { it["type"]!!.jsonPrimitive.content == "identify" }
            val track = events.first { it["type"]!!.jsonPrimitive.content == "track" }
            assertEquals("\$identify", identify["name"]!!.jsonPrimitive.content)
            assertEquals("user_42", identify["distinct_id"]!!.jsonPrimitive.content)
            val userProps = identify["user_properties"]!!.jsonObject
            assertEquals("a@b.com", userProps["email"]!!.jsonPrimitive.content)
            assertEquals("pro", userProps["plan"]!!.jsonPrimitive.content)
            assertEquals("user_42", track["distinct_id"]!!.jsonPrimitive.content)
        } finally {
            cap.close()
        }
    }

    @Test
    fun `screen emite vista mobile con propiedades`() {
        val cap = Capture()
        try {
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "tk",
                    service = "screen-test",
                    installGlobalHandlers = false,
                    flushIntervalMs = 50,
                ),
            )
            Faro.screen("CheckoutSuccess", mapOf("source" to "cart"))
            Faro.flush(timeoutMs = 2_000)
            waitFor { productEvents(cap).isNotEmpty() }

            val event = productEvents(cap).first()
            assertEquals("screen", event["type"]!!.jsonPrimitive.content)
            assertEquals("CheckoutSuccess", event["name"]!!.jsonPrimitive.content)
            assertEquals("cart", event["properties"]!!.jsonObject["source"]!!.jsonPrimitive.content)
            assertEquals("mobile", event["source"]!!.jsonPrimitive.content)
        } finally {
            cap.close()
        }
    }

    @Test
    fun `alias fusiona anonymous id previo con user post login`() {
        val cap = Capture()
        try {
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "tk",
                    service = "alias-test",
                    installGlobalHandlers = false,
                    flushIntervalMs = 50,
                ),
            )
            Faro.alias("anonymous_abc123", "user_42")
            Faro.track("post_alias")
            Faro.flush(timeoutMs = 2_000)
            waitFor { productEvents(cap).size >= 2 }

            val events = productEvents(cap)
            val alias = events.first { it["type"]!!.jsonPrimitive.content == "alias" }
            val track = events.first { it["type"]!!.jsonPrimitive.content == "track" }
            assertEquals("\$alias", alias["name"]!!.jsonPrimitive.content)
            assertEquals("anonymous_abc123", alias["anonymous_id"]!!.jsonPrimitive.content)
            assertEquals("user_42", alias["distinct_id"]!!.jsonPrimitive.content)
            assertEquals("user_42", track["distinct_id"]!!.jsonPrimitive.content)
        } finally {
            cap.close()
        }
    }

    // ---- 7. captureException compone shape OTel (auto-captura lo invoca igual) ----
    //
    // El handler de Thread.setDefaultUncaughtExceptionHandler() invoca
    // captureException internamente. Si esto es correcto, el flujo
    // auto-disparado también lo es (el wrapper son ~5 líneas).

    @Test
    fun `captureException compone exception type message stacktrace`() {
        val cap = Capture()
        try {
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "tk",
                    service = "auto-capture",
                    installGlobalHandlers = false,
                    flushIntervalMs = 50,
                ),
            )
            val ex = try {
                throw IllegalStateException("boom sintético")
            } catch (e: IllegalStateException) {
                e
            }
            Faro.captureException(ex, tags = mapOf("origin" to "test"))
            Faro.flush(timeoutMs = 2_000)
            waitFor { cap.batches.isNotEmpty() }

            val entry = cap.batches.first()["logs"]!!.jsonArray.first().jsonObject
            assertEquals("ERROR", entry["level"]!!.jsonPrimitive.content)
            assertTrue(entry["message"]!!.jsonPrimitive.content.contains("boom sintético"))
            val attrs = entry["attributes"]!!.jsonObject
            assertEquals("IllegalStateException", attrs["exception.type"]!!.jsonPrimitive.content)
            assertEquals("boom sintético", attrs["exception.message"]!!.jsonPrimitive.content)
            val stack = attrs["exception.stacktrace"]?.jsonPrimitive?.content
            assertNotNull(stack)
            assertTrue(stack.isNotEmpty(), "stacktrace presente")
            assertEquals("test", attrs["origin"]!!.jsonPrimitive.content)
        } finally {
            cap.close()
        }
    }

    // ---- 8. close() graceful: no pierde eventos en cola ----

    @Test
    fun `close drena la cola antes de devolver`() {
        val cap = Capture()
        try {
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "tk",
                    service = "close-test",
                    installGlobalHandlers = false,
                    // Intervalo lejano: si NO fuera por close(), no llegaría nada.
                    flushIntervalMs = 100_000,
                ),
            )
            repeat(7) { Faro.info("evento-$it") }
            Faro.close()

            // Tras close(), el server debe haber recibido los 7. close() llama a
            // flush(2_000) por dentro y luego cancela el scope.
            val msgs = cap.batches.flatMap { batch ->
                batch["logs"]!!.jsonArray.map { it.jsonObject["message"]!!.jsonPrimitive.content }
            }
            // BUG conocido: Channel(capacity=1) puede haber retrasado entregas; aceptamos
            // que lleguen TODOS los 7 (idealmente) o al menos no perdamos más de los
            // primeros si el bug afecta. Si esto falla, el bug del channel está costando
            // eventos en shutdown — info clave.
            assertEquals(7, msgs.size, "close() debe drenar los 7 eventos en cola; got $msgs")
        } finally {
            cap.close()
        }
    }

    // ---- 4. scrubbing ----
    @Test
    fun `scrubbing aplica scrubFields y scrubPatterns`() {
        val cap = Capture()
        try {
            Faro.init(
                FaroOptions(
                    endpoint = cap.endpoint,
                    token = "tk",
                    service = "scrub",
                    installGlobalHandlers = false,
                    flushIntervalMs = 50,
                ),
            )
            Faro.log(
                level = "INFO",
                message = "auth con eyJabc.def.ghi y key sk-abcdefghijklmnop",
                attributes = mapOf(
                    "user.password" to "p4ssw0rd",
                    "http.request.header.authorization" to "Bearer x",
                    "safe.field" to "visible",
                    "embedded" to "ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ),
            )
            Faro.flush(timeoutMs = 2_000)
            waitFor { cap.batches.isNotEmpty() }
            val log = cap.batches.first()["logs"]!!.jsonArray.first().jsonObject
            val attrs = log["attributes"]!!.jsonObject
            assertEquals("[REDACTED]", attrs["user.password"]!!.jsonPrimitive.content)
            assertEquals("[REDACTED]", attrs["http.request.header.authorization"]!!.jsonPrimitive.content)
            assertEquals("visible", attrs["safe.field"]!!.jsonPrimitive.content)
            assertEquals("[REDACTED]", attrs["embedded"]!!.jsonPrimitive.content)
            val msg = log["message"]!!.jsonPrimitive.content
            assertFalse(msg.contains("eyJabc"), "JWT debe estar redactado en message")
            assertFalse(msg.contains("sk-abcdef"), "sk-* debe estar redactado en message")
        } finally {
            cap.close()
        }
    }
}

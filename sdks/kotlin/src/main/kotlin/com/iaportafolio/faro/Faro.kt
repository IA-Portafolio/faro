@file:Suppress("unused")

package com.iaportafolio.faro

import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.io.OutputStreamWriter
import java.net.HttpURLConnection
import java.net.URL
import java.nio.charset.StandardCharsets
import java.time.Instant
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.random.Random

/**
 * SDK de Faro para Kotlin (Android + JVM).
 *
 * ```kotlin
 * Faro.init(FaroOptions(
 *     endpoint = "https://faro.iaportafolio.com",
 *     token = BuildConfig.FARO_TOKEN,
 *     service = "android-app",
 *     environment = "production",
 *     release = BuildConfig.VERSION_NAME,
 * ))
 *
 * Faro.info("login ok", mapOf("userId" to "42"))
 *
 * try { pay() } catch (e: Throwable) {
 *     Faro.captureException(e, mapOf("flow" to "checkout"))
 *     throw e
 * }
 * ```
 */
data class FaroOptions(
    val endpoint: String,
    val token: String,
    val service: String,
    val environment: String? = null,
    val release: String? = null,
    val attributes: Map<String, String> = emptyMap(),
    // Perfil de defaults: "mobile" (sdks/README.md → Perfiles de defaults).
    val flushIntervalMs: Long = 1500,
    val maxBatchSize: Int = 100,
    val maxQueueSize: Int = 5000,
    val installGlobalHandlers: Boolean = true,
    val httpTimeoutMs: Int = 8000,
    /** Substrings case-insensitive: cualquier atributo cuya clave los contenga se redacta antes de salir. */
    val scrubFields: List<String> = DEFAULT_SCRUB_FIELDS,
    /** Si true, suma headers comunes (authorization, cookie, set-cookie) a scrubFields. */
    val scrubHeaders: Boolean = true,
    /** Presets aplicados a values string y al message. Válidos: "email","jwt","credit-card","api-key". */
    val scrubPatterns: List<String> = listOf("jwt", "api-key"),
    /** Hook post-scrub; devolver null descarta el evento. */
    val beforeSend: ((WireEntry) -> WireEntry?)? = null,
)

/** Payload exacto que sale por la red (post-merge de atributos + post-scrub).
 *  Es lo que recibe [FaroOptions.beforeSend] y lo que se puede modificar/descartar. */
@Serializable
data class WireEntry(
    val level: String,
    val message: String,
    val timestamp: String,
    val attributes: Map<String, String>,
    val trace_id: String? = null,
    val span_id: String? = null,
)

val DEFAULT_SCRUB_FIELDS: List<String> = listOf(
    "password", "token", "secret", "authorization", "cookie", "set-cookie", "api_key", "apikey",
)
private val HEADER_SCRUB_FIELDS = listOf("authorization", "cookie", "set-cookie")
private const val REDACTED = "[REDACTED]"

private val SCRUB_REGEXES: Map<String, Regex> = mapOf(
    "email" to Regex("""[\w.+-]+@[\w-]+(?:\.[\w-]+)+"""),
    "jwt" to Regex("""\beyJ[\w-]+\.[\w-]+\.[\w-]+\b"""),
    // Sin Luhn; opt-in deliberadamente.
    "credit-card" to Regex("""\b(?:\d[ -]?){13,19}\b"""),
    "api-key" to Regex("""\b(?:sk-|ghp_|ghs_|gho_|github_pat_|xoxb-|xoxp-|xoxs-|AKIA|ASIA|AIza)[\w-]{12,}\b"""),
)

@Serializable
private data class WireBatch(val service: String, val logs: List<WireEntry>)

/** Payload de un product event. Las propiedades viajan como JsonObject para
 *  preservar tipos (numbers/booleans/strings/nested) — el SDK Node/Python lo
 *  manda como objeto crudo y queremos paridad. */
@Serializable
data class ProductEventWire(
    val type: String,
    val name: String,
    val timestamp: String,
    val distinct_id: String,
    val anonymous_id: String,
    val session_id: String,
    val properties: JsonObject,
    val user_properties: JsonObject,
    val context: JsonObject,
    val source: String,
)

@Serializable
private data class ProductEventBatch(val service: String, val events: List<ProductEventWire>)

object Faro {
    private val json = Json { encodeDefaults = false; ignoreUnknownKeys = true }
    // scope y channel son `var`: tras close() el SupervisorJob queda cancelado y
    // el channel cerrado, así que init() los re-crea para soportar re-init
    // (tests, hot-reload en dev, fixture multi-tenant).
    private var scope: CoroutineScope = newScope()
    // Capacidad real se aplica en init() según FaroOptions.maxQueueSize. Lo
    // inicializamos con un valor mínimo para evitar nulls; init() lo re-crea.
    private var channel: Channel<WireEntry> = Channel(capacity = 1)
    private var eventsChannel: Channel<ProductEventWire> = Channel(capacity = 1)
    private var opts: FaroOptions? = null
    private val started = AtomicBoolean(false)
    private val closed = AtomicBoolean(false)
    private var prevUncaughtHandler: Thread.UncaughtExceptionHandler? = null
    private var scrubNeedles: List<String> = emptyList()
    private var scrubRegexesActive: List<Regex> = emptyList()

    // Estado de identidad para product events. La concurrencia es low-contention
    // (track/identify/alias se invocan típicamente desde UI, no en hot path), así
    // que @Synchronized basta sin un Mutex coroutines-aware.
    @Volatile private var distinctId: String = ""
    @Volatile private var anonymousId: String = ""
    private val userProperties: MutableMap<String, JsonElement> = mutableMapOf()
    // Lo toma el flusher mientras envía un batch HTTP; flush() lo toma también
    // para garantizar que close() no cancele el scope mid-send (los eventos del
    // batch llegan al server antes de devolver).
    private val sendMutex = Mutex()

    private fun newScope(): CoroutineScope =
        CoroutineScope(SupervisorJob() + Dispatchers.IO + CoroutineName("faro"))

    @Synchronized
    fun init(options: FaroOptions) {
        // Paridad cross-SDK: Python/Go/Node/Flutter/Expo lanzan un error claro en el mismo caso.
        require(options.endpoint.isNotEmpty()) { "faro.init: 'endpoint' es obligatorio (string no vacío)" }
        require(options.token.isNotEmpty()) { "faro.init: 'token' es obligatorio (string no vacío)" }
        require(options.service.isNotEmpty()) { "faro.init: 'service' es obligatorio (string no vacío)" }
        if (started.get()) close()
        // Tras un close() previo el scope quedó cancelado y el channel cerrado;
        // un re-init debe arrancar runtime fresco. Recreamos también el channel
        // SIEMPRE para que respete el maxQueueSize del init actual (antes esto
        // estaba hardcoded a capacity=1, que perdía eventos bajo carga).
        if (scope.coroutineContext[Job]?.isActive != true) scope = newScope()
        if (!channel.isClosedForSend) channel.close()
        if (!eventsChannel.isClosedForSend) eventsChannel.close()
        channel = Channel(capacity = options.maxQueueSize)
        eventsChannel = Channel(capacity = options.maxQueueSize)
        opts = options.copy(endpoint = options.endpoint.trimEnd('/'))
        val needles = options.scrubFields.map { it.lowercase() }.toMutableSet()
        if (options.scrubHeaders) needles.addAll(HEADER_SCRUB_FIELDS)
        scrubNeedles = needles.toList()
        scrubRegexesActive = options.scrubPatterns.mapNotNull { SCRUB_REGEXES[it] }
        // anonymous_id estable durante el lifetime del proceso. Regenera tras
        // re-init; en una fase 2 lo persistiremos en SharedPreferences para
        // poder hacer alias() entre runs.
        anonymousId = "anon_${System.nanoTime().toString(36)}_${Random.nextLong().toString(36).take(8)}"
        synchronized(userProperties) { userProperties.clear() }
        distinctId = ""
        started.set(true)
        closed.set(false)
        scope.launch { runFlusher() }
        scope.launch { runEventsFlusher() }
        if (options.installGlobalHandlers) installHandlers()
    }

    fun log(
        level: String = "INFO",
        message: String,
        attributes: Map<String, Any?> = emptyMap(),
        traceId: String? = null,
        spanId: String? = null,
    ) {
        val o = opts ?: return
        if (closed.get()) return
        val attrs = buildMap<String, String> {
            putAll(o.attributes)
            o.environment?.let { put("deployment.environment", it) }
            o.release?.let { put("service.version", it) }
            for ((k, v) in attributes) put(k, v?.toString() ?: "")
        }
        val rawEntry = WireEntry(
            level = level.uppercase(),
            message = message,
            timestamp = Instant.now().toString(),
            attributes = attrs,
            trace_id = traceId,
            span_id = spanId,
        )
        val scrubbed = scrub(rawEntry)
        // Paridad cross-SDK: beforeSend devolviendo null descarta el evento.
        // Antes el ?: scrubbed rescataba el null y enviaba igual — bug crítico
        // para usuarios que dependieran de beforeSend para muestrear o filtrar PII.
        val finalEntry = if (o.beforeSend != null) {
            o.beforeSend.invoke(scrubbed) ?: return
        } else {
            scrubbed
        }
        val result = channel.trySend(finalEntry)
        if (result.isFailure) {
            System.err.println("[faro] cola llena, evento descartado")
        }
    }

    private fun scrub(e: WireEntry): WireEntry {
        val newAttrs = LinkedHashMap<String, String>(e.attributes.size)
        for ((k, v) in e.attributes) {
            val kLower = k.lowercase()
            newAttrs[k] = when {
                scrubNeedles.any { kLower.contains(it) } -> REDACTED
                scrubRegexesActive.isNotEmpty() -> scrubRegexesActive.fold(v) { acc, rx -> rx.replace(acc, REDACTED) }
                else -> v
            }
        }
        val newMessage = if (scrubRegexesActive.isNotEmpty())
            scrubRegexesActive.fold(e.message) { acc, rx -> rx.replace(acc, REDACTED) }
        else e.message
        return e.copy(attributes = newAttrs, message = newMessage)
    }

    fun info(message: String, attrs: Map<String, Any?> = emptyMap()) = log("INFO", message, attrs)
    fun warn(message: String, attrs: Map<String, Any?> = emptyMap()) = log("WARN", message, attrs)
    /** Alias de [warn] — paridad con `logging.WARNING` / SDK de Python. */
    fun warning(message: String, attrs: Map<String, Any?> = emptyMap()) = log("WARN", message, attrs)
    fun error(message: String, attrs: Map<String, Any?> = emptyMap()) = log("ERROR", message, attrs)

    fun captureException(
        throwable: Throwable,
        tags: Map<String, String> = emptyMap(),
        message: String? = null,
    ) {
        val sw = java.io.StringWriter()
        throwable.printStackTrace(java.io.PrintWriter(sw))
        val attrs = buildMap<String, Any?> {
            put("exception.type", throwable.javaClass.simpleName)
            put("exception.message", throwable.message ?: "")
            put("exception.stacktrace", sw.toString())
            putAll(tags)
        }
        log("ERROR", message ?: "${throwable.javaClass.simpleName}: ${throwable.message}", attrs)
    }

    // ---------- Product events API (Segment/PostHog-like) ----------

    fun track(eventName: String, properties: Map<String, Any?> = emptyMap()) {
        enqueueEvent(type = "track", name = eventName, properties = properties)
    }

    fun identify(userId: String, traits: Map<String, Any?> = emptyMap()) {
        if (userId.isEmpty()) return
        distinctId = userId
        if (traits.isNotEmpty()) {
            synchronized(userProperties) {
                for ((k, v) in traits) userProperties[k] = jsonOf(v)
            }
        }
        enqueueEvent(
            type = "identify",
            name = "\$identify",
            properties = emptyMap(),
            userPropertiesOverride = traits,
        )
    }

    /** Mobile-only: marca una transición de pantalla. Paridad con la API Expo/Flutter. */
    fun screen(screenName: String, properties: Map<String, Any?> = emptyMap()) {
        enqueueEvent(type = "screen", name = screenName, properties = properties)
    }

    fun alias(prevId: String, newId: String) {
        if (prevId.isEmpty() || newId.isEmpty()) return
        distinctId = newId
        enqueueEvent(
            type = "alias",
            name = "\$alias",
            properties = emptyMap(),
            anonymousIdOverride = prevId,
        )
    }

    private fun enqueueEvent(
        type: String,
        name: String,
        properties: Map<String, Any?>,
        userPropertiesOverride: Map<String, Any?>? = null,
        anonymousIdOverride: String? = null,
    ) {
        val o = opts ?: return
        if (closed.get()) return
        val ctx = buildMap<String, JsonElement> {
            for ((k, v) in o.attributes) put(k, JsonPrimitive(v))
            o.environment?.let { put("environment", JsonPrimitive(it)) }
            o.release?.let { put("release", JsonPrimitive(it)) }
        }
        val userPropsJson = if (userPropertiesOverride != null) {
            userPropertiesOverride.mapValues { jsonOf(it.value) }
        } else {
            synchronized(userProperties) { userProperties.toMap() }
        }
        val event = ProductEventWire(
            type = type,
            name = name,
            timestamp = Instant.now().toString(),
            distinct_id = distinctId.ifEmpty { anonymousId },
            anonymous_id = anonymousIdOverride ?: anonymousId,
            session_id = "",
            properties = JsonObject(properties.mapValues { jsonOf(it.value) }),
            user_properties = JsonObject(userPropsJson),
            context = JsonObject(ctx),
            source = "mobile",
        )
        val result = eventsChannel.trySend(event)
        if (result.isFailure) {
            System.err.println("[faro] cola de events llena, evento descartado")
        }
    }

    /** Conversión barata Any? → JsonElement para mantener tipos primitivos.
     *  Lo desconocido (objetos custom, listas) cae a `toString()` — los
     *  consumidores del 99% de los casos pasan numbers/strings/booleans. */
    private fun jsonOf(v: Any?): JsonElement = when (v) {
        null -> JsonNull
        is Number -> JsonPrimitive(v)
        is Boolean -> JsonPrimitive(v)
        is String -> JsonPrimitive(v)
        else -> JsonPrimitive(v.toString())
    }

    fun flush(timeoutMs: Long = 3000) {
        runBlocking {
            withTimeoutOrNull(timeoutMs) {
                // Drena cediendo el control hasta que ambos flushers se pongan al día.
                while (!channel.isEmpty || !eventsChannel.isEmpty) delay(50)
                // Esperar a que el batch en vuelo (si lo hay) termine. Sin esto
                // close() podría cancelar el scope a mitad del HTTP POST.
                sendMutex.withLock { /* solo nos importa adquirir y soltar */ }
            }
        }
    }

    fun close() {
        if (!closed.compareAndSet(false, true)) return
        flush(timeoutMs = 2000)
        scope.cancel("cierre de faro")
        // Cierra ambos channels para que el próximo init() detecte estado
        // terminal y cree uno nuevo. Sin esto, init() reusaría los canales
        // viejos cuyos únicos consumidores (los flushers) están cancelados.
        channel.close()
        eventsChannel.close()
        prevUncaughtHandler?.let { Thread.setDefaultUncaughtExceptionHandler(it) }
        prevUncaughtHandler = null
        started.set(false)
    }

    // ---------- internal ----------

    private suspend fun runFlusher() {
        val o = opts ?: return
        val batch = ArrayList<WireEntry>(o.maxBatchSize)
        while (true) {
            // Espera o bien una entrada o bien el intervalo de flush.
            val first = withTimeoutOrNull(o.flushIntervalMs) { channel.receive() }
            if (first != null) batch += first
            // Drena cualquier otra cosa que haya llegado mientras tanto.
            while (batch.size < o.maxBatchSize) {
                val next = channel.tryReceive().getOrNull() ?: break
                batch += next
            }
            if (batch.isNotEmpty()) {
                sendMutex.withLock { send(o, batch) }
                batch.clear()
            }
            if (closed.get() && channel.isEmpty) return
        }
    }

    private suspend fun runEventsFlusher() {
        val o = opts ?: return
        val batch = ArrayList<ProductEventWire>(o.maxBatchSize)
        while (true) {
            val first = withTimeoutOrNull(o.flushIntervalMs) { eventsChannel.receive() }
            if (first != null) batch += first
            while (batch.size < o.maxBatchSize) {
                val next = eventsChannel.tryReceive().getOrNull() ?: break
                batch += next
            }
            if (batch.isNotEmpty()) {
                sendMutex.withLock { sendEvents(o, batch) }
                batch.clear()
            }
            if (closed.get() && eventsChannel.isEmpty) return
        }
    }

    private fun sendEvents(o: FaroOptions, batch: List<ProductEventWire>) {
        val body = json.encodeToString(ProductEventBatch(o.service, batch))
        var networkFailed = false
        var status = 0
        try {
            val url = URL("${o.endpoint}/api/v1/ingest/events")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "POST"
                doOutput = true
                connectTimeout = o.httpTimeoutMs
                readTimeout = o.httpTimeoutMs
                setRequestProperty("Authorization", "Bearer ${o.token}")
                setRequestProperty("Content-Type", "application/json")
            }
            OutputStreamWriter(conn.outputStream, StandardCharsets.UTF_8).use { it.write(body) }
            status = conn.responseCode
            if (status >= 400) {
                System.err.println("[faro] ingest events HTTP $status")
            }
            conn.disconnect()
        } catch (t: Throwable) {
            System.err.println("[faro] falló el flush de events: ${t.message}")
            networkFailed = true
        }
        val shouldRetry = networkFailed || status in 500..599
        if (!shouldRetry) return
        for (entry in batch) {
            val result = eventsChannel.trySend(entry)
            if (result.isFailure) {
                System.err.println("[faro] cola de events llena al reintentar, evento descartado")
            }
        }
    }

    private fun send(o: FaroOptions, batch: List<WireEntry>) {
        val body = json.encodeToString(WireBatch(o.service, batch))
        var networkFailed = false
        var status = 0
        try {
            val url = URL("${o.endpoint}/api/v1/ingest/logs")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "POST"
                doOutput = true
                connectTimeout = o.httpTimeoutMs
                readTimeout = o.httpTimeoutMs
                setRequestProperty("Authorization", "Bearer ${o.token}")
                setRequestProperty("Content-Type", "application/json")
            }
            OutputStreamWriter(conn.outputStream, StandardCharsets.UTF_8).use { it.write(body) }
            status = conn.responseCode
            if (status >= 400) {
                System.err.println("[faro] ingest HTTP $status")
            }
            conn.disconnect()
        } catch (t: Throwable) {
            System.err.println("[faro] falló el flush: ${t.message}")
            networkFailed = true
        }
        // Paridad cross-SDK: 5xx o fallo de red → re-encolar para reintentar
        // en el siguiente tick. 4xx descartamos (batch malformado / auth inválida).
        // Antes este SDK descartaba SIEMPRE — perdíamos eventos ante 5xx transitorio.
        val shouldRetry = networkFailed || status in 500..599
        if (!shouldRetry) return
        for (entry in batch) {
            val result = channel.trySend(entry)
            if (result.isFailure) {
                System.err.println("[faro] cola llena al reintentar, evento descartado")
            }
        }
    }

    private fun installHandlers() {
        prevUncaughtHandler = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            try {
                captureException(throwable, tags = mapOf("thread" to thread.name), message = "[uncaught] ${throwable.javaClass.simpleName}")
                flush(timeoutMs = 1500)
            } finally {
                prevUncaughtHandler?.uncaughtException(thread, throwable)
            }
        }
    }
}

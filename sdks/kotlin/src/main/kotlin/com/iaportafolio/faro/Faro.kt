@file:Suppress("unused")

package com.iaportafolio.faro

import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.io.OutputStreamWriter
import java.net.HttpURLConnection
import java.net.URL
import java.nio.charset.StandardCharsets
import java.time.Instant
import java.util.concurrent.atomic.AtomicBoolean

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
    val flushIntervalMs: Long = 1500,
    val maxBatchSize: Int = 100,
    val maxQueueSize: Int = 5000,
    val installGlobalHandlers: Boolean = true,
    val httpTimeoutMs: Int = 8000,
)

@Serializable
private data class WireEntry(
    val level: String,
    val message: String,
    val timestamp: String,
    val attributes: Map<String, String>,
    val trace_id: String? = null,
    val span_id: String? = null,
)

@Serializable
private data class WireBatch(val service: String, val logs: List<WireEntry>)

object Faro {
    private val json = Json { encodeDefaults = false; ignoreUnknownKeys = true }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO + CoroutineName("faro"))
    private val channel = Channel<WireEntry>(capacity = 1)
    private var opts: FaroOptions? = null
    private val started = AtomicBoolean(false)
    private val closed = AtomicBoolean(false)
    private var prevUncaughtHandler: Thread.UncaughtExceptionHandler? = null

    @Synchronized
    fun init(options: FaroOptions) {
        if (started.get()) close()
        opts = options.copy(endpoint = options.endpoint.trimEnd('/'))
        started.set(true)
        closed.set(false)
        scope.launch { runFlusher() }
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
        val entry = WireEntry(
            level = level.uppercase(),
            message = message,
            timestamp = Instant.now().toString(),
            attributes = attrs,
            trace_id = traceId,
            span_id = spanId,
        )
        val result = channel.trySend(entry)
        if (result.isFailure) {
            System.err.println("[faro] cola llena, evento descartado")
        }
    }

    fun info(message: String, attrs: Map<String, Any?> = emptyMap()) = log("INFO", message, attrs)
    fun warn(message: String, attrs: Map<String, Any?> = emptyMap()) = log("WARN", message, attrs)
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

    fun flush(timeoutMs: Long = 3000) {
        runBlocking {
            withTimeoutOrNull(timeoutMs) {
                // Drena cediendo el control hasta que el flusher se ponga al día. El flusher
                // ve el canal vacío y se queda inactivo, así que una espera corta basta.
                while (!channel.isEmpty) delay(50)
            }
        }
    }

    fun close() {
        if (!closed.compareAndSet(false, true)) return
        flush(timeoutMs = 2000)
        scope.cancel("cierre de faro")
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
                send(o, batch)
                batch.clear()
            }
            if (closed.get() && channel.isEmpty) return
        }
    }

    private fun send(o: FaroOptions, batch: List<WireEntry>) {
        val body = json.encodeToString(WireBatch(o.service, batch))
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
            val code = conn.responseCode
            if (code >= 400) {
                System.err.println("[faro] ingest HTTP $code")
            }
            conn.disconnect()
        } catch (t: Throwable) {
            System.err.println("[faro] falló el flush: ${t.message}")
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

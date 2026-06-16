# faro (Kotlin · Android + JVM)

> **Perfil de defaults:** `mobile` — flush 1500ms · batch 100 · queue 5 000. Ver [perfiles](../README.md#perfiles-de-defaults).

Para Android añade al `build.gradle.kts` del módulo:

```kotlin
dependencies {
    implementation("com.iaportafolio:faro:0.1.0")
}
```

```kotlin
import com.iaportafolio.faro.Faro
import com.iaportafolio.faro.FaroOptions

class App : Application() {
    override fun onCreate() {
        super.onCreate()
        Faro.init(FaroOptions(
            endpoint = "https://faro.iaportafolio.com",
            token = BuildConfig.FARO_TOKEN,
            service = "android-app",
            environment = "production",
            release = BuildConfig.VERSION_NAME,
            attributes = mapOf("device.brand" to Build.BRAND),
        ))
    }
}

// uso
Faro.info("login ok", mapOf("user_id" to user.id))

try { pay() } catch (e: Throwable) {
    Faro.captureException(e, mapOf("flow" to "checkout"))
    throw e
}
```

## Captura automática

`init()` instala un `Thread.setDefaultUncaughtExceptionHandler` que reporta la excepción a Faro y luego delega al handler previo (no rompe Crashlytics ni el reporte por defecto de Android).

Para corutinas, usa un `CoroutineExceptionHandler` propio:

```kotlin
val exHandler = CoroutineExceptionHandler { ctx, ex ->
    Faro.captureException(ex, mapOf("scope" to ctx[CoroutineName]?.name.orEmpty()))
}
scope.launch(exHandler) { /* ... */ }
```

## Permisos en Android

Asegúrate de tener `INTERNET` en el manifest (suele estar por defecto):

```xml
<uses-permission android:name="android.permission.INTERNET" />
```

## Product analytics

```kotlin
// Eventos de producto
Faro.track("checkout_completed", mapOf("amount" to 99.50, "currency" to "USD"))

// Identificar usuario
Faro.identify("user_42", mapOf("email" to "a@b.com", "plan" to "pro"))

// Fusionar sesión anónima con usuario post-login
Faro.alias("anon_abc123", "user_42")
```

Ver [API uniforme](../README.md#api-uniforme-entre-sdks) para la semántica de
`anonymous_id`/`distinct_id`/`session_id`.

## Opciones de init

| Opción | Default | Descripción |
| ------ | ------- | ----------- |
| `flushIntervalMs` | `1500` | Cadencia de flush (ms). |
| `maxBatchSize` | `100` | Eventos por POST. |
| `maxQueueSize` | `5000` | Cap de la cola. Al llenarse descarta el más viejo. |
| `installGlobalHandlers` | `true` | Instala `Thread.setDefaultUncaughtExceptionHandler`. |
| `httpTimeoutMs` | `8000` | Timeout de connect + read para HTTP. |

```kotlin
Faro.init(FaroOptions(
    endpoint = "...", token = "...", service = "android-app",
    flushIntervalMs = 3000,
    installGlobalHandlers = false,
    httpTimeoutMs = 5000,
))
```

## Opciones cross-SDK

`Faro.warning()` (alias de `warn()`), `scrubFields`/`scrubHeaders`/`scrubPatterns` y el hook `beforeSend: (WireEntry) -> WireEntry?` están disponibles con la misma semántica que en el resto de SDKs. Ver [API uniforme entre SDKs](../README.md#api-uniforme-entre-sdks).

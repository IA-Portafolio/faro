# faro (Kotlin · Android + JVM)

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

# faro_sdk (Flutter / Dart)

> **Perfil de defaults:** `mobile` — flush 1500ms · batch 100 · queue 5 000. Ver [perfiles](../README.md#perfiles-de-defaults).

```yaml
dependencies:
  faro_sdk: ^0.1.0
```

```dart
import 'package:flutter/material.dart';
import 'package:faro_sdk/faro_sdk.dart';

void main() {
  Faro.run(
    options: const FaroOptions(
      endpoint: 'https://faro.iaportafolio.com',
      token: '...',                       // /projects → SDK
      service: 'mi-app-mobile',
      environment: 'production',
      release: '1.4.2+213',
    ),
    appRunner: () => runApp(const MyApp()),
  );
}

// más adelante…
Faro.instance.info('login completado', {'user_id': 42});

try {
  await pagar();
} catch (e, st) {
  Faro.instance.captureException(e, stack: st, tags: {'flow': 'checkout'});
  rethrow;
}
```

## Captura automática

`Faro.run(...)` instala:

- `FlutterError.onError` — para errores dentro del framework (build/layout/paint).
- `PlatformDispatcher.instance.onError` — para errores que escapan del framework.
- `runZonedGuarded` — para errores asincrónicos en la zona del `appRunner`.

Si no quieres esos handlers, llama a `Faro.init(options)` directamente (sin `.run`).

## Flush al fondo

`Faro.run(...)` instala automáticamente un `WidgetsBindingObserver` que hace flush cuando la app pasa a `paused` / `hidden` / `detached`. No tienes que hacer nada.

Si llamas a `Faro.init(...)` directo (sin `.run`), el observer no se instala — o lo agregas tú, o haces flush manual desde tu propio observer:

```dart
@override
void didChangeAppLifecycleState(AppLifecycleState state) {
  if (state == AppLifecycleState.paused) Faro.instance.flush();
}
```

## Opciones cross-SDK

`warning()` (alias de `warn()`), `scrubFields`/`scrubHeaders`/`scrubPatterns` y el hook `beforeSend` están disponibles con la misma semántica que en el resto de SDKs. Ver [API uniforme entre SDKs](../README.md#api-uniforme-entre-sdks).

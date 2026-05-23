# faro_sdk (Flutter / Dart)

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

`flutter` no expone un evento global de "app cerrándose" en todas las plataformas, así que el SDK hace flush periódico. Para asegurar persistencia en transiciones de ciclo de vida, llama manualmente desde un `WidgetsBindingObserver`:

```dart
@override
void didChangeAppLifecycleState(AppLifecycleState state) {
  if (state == AppLifecycleState.paused) Faro.instance.flush();
}
```

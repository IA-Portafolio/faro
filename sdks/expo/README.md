# @iaportafolio/expo

SDK para Expo / React Native. Sin módulos nativos — funciona en Expo Go sin development build.

> **Perfil de defaults:** `mobile` — flush 2500ms · batch 80 · queue 2 000. Defaults un poco más conservadores que el baseline `mobile` (1500ms · 100 · 5 000) por el coste del bridge JS↔nativo y batería. Ver [perfiles](../README.md#perfiles-de-defaults).

```bash
npx expo install @iaportafolio/expo
```

```tsx
// App.tsx
import { useEffect } from 'react';
import * as faro from '@iaportafolio/expo';

faro.init({
  endpoint: 'https://faro.iaportafolio.com',
  token: process.env.EXPO_PUBLIC_FARO_TOKEN!,
  service: 'mi-app-mobile',
  environment: __DEV__ ? 'dev' : 'production',
  release: '1.4.2',
});

export default function App() {
  useEffect(() => {
    faro.info('app montada');
  }, []);
  // ...
}

// más adelante
try {
  await pagar();
} catch (err) {
  faro.captureException(err, { tags: { flow: 'checkout' } });
  throw err;
}
```

## Captura automática

`init()` se engancha a `ErrorUtils.setGlobalHandler`, lo que cubre tanto errores de render como excepciones asíncronas en el hilo JS. El handler previo (el rojo de pantalla de RN o el Error Boundary de Expo) sigue corriendo después de Faro.

Para `unhandledrejection` en promesas, usa el polyfill estándar de RN (ya incluido); el SDK los recoge como cualquier otro error.

## Token expuesto

El token vive en el bundle. Es **deliberado**: el token de ingesta solo permite ENVIAR logs, no leer datos del dashboard. Si lo necesitas rotar (por ejemplo si lo filtran), entra a `/projects` en el dashboard de Faro y pulsa **Rotar token**.

## Flush al fondo

El SDK instala automáticamente un listener de `AppState` (vía `installGlobalHandlers: true`, por defecto): al pasar a `background` o `inactive` se hace flush y, si queda algo, se persiste a AsyncStorage. No tienes que hacer nada.

Si prefieres flushear manualmente desde tu propio handler, basta con:

```tsx
import { AppState } from 'react-native';
AppState.addEventListener('change', (state) => {
  if (state === 'background') faro.flush();
});
```

## Persistencia entre sesiones (AsyncStorage)

Mobile mata apps de forma agresiva (memory pressure, swipe del task switcher, OOM). Si la cola de Faro vive solo en memoria, se pierde todo lo que no haya salido. El SDK persiste **automáticamente** en AsyncStorage si tienes el peer dep:

```bash
npx expo install @react-native-async-storage/async-storage
```

Sin más configuración:

- **Al pasar a background / inactive**: flush → si queda algo (red caída, server 5xx), persiste a `@faro/queue/{service}`.
- **En el próximo `init()`**: carga lo persistido, lo prepende a la cola y dispara flush inmediato.
- **Tras un fatal**: persiste antes de propagar al handler previo (best-effort — Android es más fiable que iOS aquí).
- **TTL de 24h**: eventos más viejos se descartan en lugar de inundar el servidor con logs rancios cuando la app se reabre tras una semana.

Personalización opcional:

```tsx
faro.init({
  endpoint, token, service,
  persistence: {
    ttlMs: 6 * 60 * 60 * 1000,  // 6h — apps con sesiones cortas
    maxBytes: 64 * 1024,         // 64 KB — apps con muchos atributos por evento
    key: '@miapp/faro-queue',    // si necesitas convivir con otra instalación
  },
});

// O desactivar por completo:
faro.init({ endpoint, token, service, persistence: false });
```

Si `@react-native-async-storage/async-storage` no está instalado, el SDK **funciona igual** — solo pierde la cola al matar la app. No hay error, no hay warning ruidoso: simplemente la persistencia queda apagada.

# @iaportafolio/expo

SDK para Expo / React Native. Sin módulos nativos — funciona en Expo Go sin development build.

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

```tsx
import { AppState } from 'react-native';
AppState.addEventListener('change', (state) => {
  if (state === 'background') faro.flush();
});
```

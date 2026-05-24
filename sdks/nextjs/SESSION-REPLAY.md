# Session Replay

Graba el DOM del usuario como una secuencia de eventos rrweb y los reproduce dentro de Faro. Cuando alguien reporta un bug, abres el error en el dashboard y **ves su pantalla** — clicks, scrolls, inputs (enmascarados) y navegaciones del momento exacto en que se rompió.

> Opt-in. Cuesta ancho de banda y privacy budget — no se activa solo.

---

## Activar

1. Instala `rrweb` como dependencia normal del proyecto consumidor. En el SDK está declarado como peer opcional, así que sin esto el SDK sigue funcionando pero el replay queda silencioso.

   ```bash
   npm install rrweb
   ```

2. Pasa `captureSessionReplay: true` al inicializar el cliente:

   ```tsx
   // app/faro-client.tsx
   'use client';
   import { useEffect } from 'react';
   import { initFaroClient } from '@iaportafolio/nextjs/client';

   export function FaroClient() {
     useEffect(() => {
       initFaroClient({
         endpoint: process.env.NEXT_PUBLIC_FARO_ENDPOINT!,
         token:    process.env.NEXT_PUBLIC_FARO_TOKEN!,
         service:  'mi-next-app-web',
         captureSessionReplay: true,
         sessionReplaySampleRate: 0.2, // 20% de sesiones — default 1.0
       });
     }, []);
     return null;
   }
   ```

3. En el dashboard, abre cualquier error. Si la sesión que disparó el error está grabada aparece el botón **▶ Ver replay**. También hay una sección "Sesiones afectadas" con un row por sesión.

---

## Auto-link con errores

El enlace entre un error y su replay no requiere instrumentación manual. Al inicializar el SDK, se genera un `session.id` por pestaña y se adjunta automáticamente a todos los eventos del browser: errores no manejados, `unhandledrejection`, `captureException`, logs, Web Vitals y breadcrumbs. Los chunks de replay viajan con el mismo `session_id`.

Cuando abres un issue de error, Faro busca los eventos de esa fingerprint que tienen `attributes['session.id']` y cruza esos ids con `faro.session_replays`. Si hay match, el dashboard muestra:

- Botón **▶ Ver replay** junto al evento afectado.
- Sección **"Sesiones afectadas"** con sesiones que reprodujeron el mismo error.
- Link directo a `/replays/<session_id>` para compartirlo con el equipo.

Esto habilita el flujo de soporte más importante: **"Show me what they did right before crashing"**. Abres el error, saltas al replay de esa misma sesión, retrocedes unos segundos en el player y ves los clicks, scrolls, navegación y estado visual que precedieron al crash.

Limitaciones esperadas:

- Si la sesión no cayó dentro de `sessionReplaySampleRate`, el error tendrá `session.id` pero no habrá replay.
- Si el replay expiró por TTL (7 días), el link puede dejar de resolver aunque el error siga existiendo.
- Si el error ocurrió antes del primer flush y el tab se cerró de inmediato, puede quedar un error con sesión pero sin eventos reproducibles.

---

## Privacidad

Por defecto el SDK enmascara lo que la mayoría de equipos consideran sensible:

| Qué | Cómo | Cambiar |
| --- | --- | --- |
| Valores de `<input>`, `<textarea>`, `<select>` | `maskAllInputs: true` | Pasa overrides a rrweb (fork del SDK) |
| Passwords, emails, búsquedas, teléfonos y otros inputs de formulario | Enmascarados por el mismo default | No requiere clase extra |
| Cualquier nodo marcado | clase CSS `.faro-mask` enmascara el texto | Añade la clase a tu UI sensible |
| `<canvas>` | No se graba — los canvases suelen contener imágenes | — |

La regla práctica: **formularios seguros por default, texto del DOM explícito por clase**. Los valores que el usuario escribe en campos de formulario salen como máscara, pero cualquier texto que renderices fuera de un input puede grabarse tal cual. Si manejas datos de tarjeta, salud, documentos legales, direcciones, nombres de clientes o identificadores internos: aplica `.faro-mask` agresivamente o no actives la grabación en esas rutas.

```tsx
<input type="email" />                                      {/* valor enmascarado */}
<input name="credit_card" autoComplete="cc-number" />        {/* valor enmascarado */}
<textarea name="support_message" />                         {/* valor enmascarado */}
<select name="plan_id" />                                   {/* valor enmascarado */}

<div className="faro-mask">{customer.fullName}</div>        {/* texto enmascarado explícitamente */}
<section className="faro-mask">
  <h2>{patient.name}</h2>
  <p>{patient.diagnosis}</p>
</section>

<div>{publicProductName}</div>                              {/* visible — está bien */}
```

Checklist recomendado antes de activar replay en producción:

- Marca con `.faro-mask` cualquier bloque que contenga PII renderizada como texto.
- No pongas secretos en `data-*`, `aria-label`, `title`, placeholders o URLs; los atributos del DOM pueden serializarse.
- Evita tokens o emails en query strings. `page_url` se guarda como metadata de cada chunk.
- Prueba un flujo real y revisa el replay resultante antes de subir el sample rate.
- Para rutas altamente sensibles, inicializa el SDK sin `captureSessionReplay` o envuélvelo con una condición por path.

> El propio paquete `rrweb` también acepta `maskTextFn`, `maskInputFn` y `blockSelector` si necesitas reglas más finas. El SDK hoy expone defaults conservadores; si tu app necesita una política configurable por selector, abre un issue y documenta qué datos quieres bloquear.

Ejemplo de gating por ruta sensible:

```tsx
useEffect(() => {
  const sensitiveRoute =
    window.location.pathname.startsWith('/billing') ||
    window.location.pathname.startsWith('/medical-records');

  initFaroClient({
    endpoint,
    token,
    service,
    captureSessionReplay: !sensitiveRoute,
    sessionReplaySampleRate: 0.1,
  });
}, []);
```

---

## Qué vio el usuario antes del crash

Para depurar un incidente, empieza desde el error y no desde la lista global de replays:

1. Abre `/errors/<fingerprint>`.
2. Entra a una sesión afectada con **▶ Ver replay**.
3. Busca el timestamp del error en la ficha del evento o en el rango de la sesión.
4. Retrocede 10-30 segundos en el player.
5. Mira el último click, navegación, scroll, cambio de formulario o render antes de la excepción.

Ese recorrido responde rápido preguntas como:

- ¿El usuario venía de una navegación interna o de una carga directa?
- ¿Había un modal, drawer o estado vacío abierto cuando falló?
- ¿El crash ocurrió después de un submit, de un cambio de filtro o de un scroll infinito?
- ¿El usuario repitió una acción varias veces antes de romper la pantalla?

Los inputs siguen enmascarados: el objetivo es entender la secuencia de interacción, no recuperar el dato exacto que escribió el usuario.

---

## Sampling

Grabar cada sesión es caro. Bajalo:

```ts
captureSessionReplay: true,
sessionReplaySampleRate: 0.1, // 10%
```

La decisión se toma **una vez por tab**, al `initFaroClient`. Si la sesión no entra en el muestreo, no se carga rrweb siquiera (ahorro de bundle vía dynamic import).

Recomendación: empieza con `0.1`-`0.2` y sube si necesitas más datos. Sesiones con error siempre van por el mismo `session.id` — el dashboard te muestra las afectadas aunque hayas sampleado bajo, mientras la sesión específica esté entre las grabadas.

---

## Session ID

El SDK genera un `session_id` (UUID v4 si está disponible, fallback `time-base36`) al primer `initFaroClient` de la pestaña. Se persiste en `sessionStorage`, así que:

- Sobrevive a F5 / navegaciones internas
- Muere al **cerrar la pestaña** (es lo que define "sesión")
- En navegación privada / Safari ITP que bloquea storage, cae a un id efímero en memoria

El id se inyecta como `attributes['session.id']` en **todos** los eventos (logs, errores, Web Vitals). El dashboard lo usa para linkear cualquier evento al replay correspondiente.

Si lo necesitas en tu código:

```ts
import { getSessionId } from '@iaportafolio/nextjs/client';

// p.ej. para incluirlo en un mensaje de soporte que el usuario pega
const sid = getSessionId();
```

---

## Qué se envía a la red

| Cosa | Cuándo |
| --- | --- |
| Snapshot completo del DOM (1er chunk + uno cada 60s) | Al cargar la página + checkpoints |
| Eventos incrementales rrweb (mutaciones, mouse, scroll, input enmascarado, viewport) | Continuo |
| Metadata del chunk: `session_id`, `service`, `seq`, `user_id`, `page_url`, `user_agent` | Cada flush |

**Cadencia de flush**: cada 5 s o cada 80 eventos, lo que ocurra primero. En `pagehide` / `visibilitychange=hidden` se hace flush con `navigator.sendBeacon` — los últimos eventos no se pierden cuando el usuario cierra la pestaña.

**Endpoint**: `POST /api/v1/ingest/replay`. Body máx 16 MiB. Auth con el mismo `Bearer <token>` del proyecto.

**Almacenamiento server-side**: tabla `faro.session_replays` en ClickHouse, columna `events` con `CODEC(ZSTD(3))` (5-10× sobre el JSON crudo). TTL de **7 días** — los replays son caros y la utilidad práctica cae rápido.

---

## Tunear

Ninguno de estos es obligatorio:

```ts
initFaroClient({
  endpoint, token, service,
  captureSessionReplay: true,

  // Probabilidad de grabar [0..1]. Default 1.0.
  sessionReplaySampleRate: 0.1,
});
```

Los demás knobs (intervalo de flush, tamaño de chunk, mousemove sampling) viven en `browser-replay.ts` con defaults pensados para una app web típica. Si necesitas modificarlos, abre un issue describiendo el caso — preferimos endurecer los defaults a multiplicar opciones de configuración.

---

## Apagar el replay sin desactivar el resto del SDK

```ts
initFaroClient({
  endpoint, token, service,
  captureSessionReplay: false, // o simplemente omitirlo
});
```

El SDK sigue capturando errores, Web Vitals y breadcrumbs normalmente.

---

## Troubleshooting

**No aparece el botón "Ver replay" en el dashboard, pero los errores sí entran.**

- Confirma que `rrweb` está instalado en el proyecto consumidor (`npm ls rrweb`). El SDK loggea en la consola del navegador `[faro] captureSessionReplay habilitado pero rrweb no se pudo cargar` cuando falla el dynamic import.
- Comprueba en la pestaña Network del browser que hay POSTs a `/api/v1/ingest/replay` y devuelven 200. Si devuelve 401, el token está mal. Si 413, el snapshot inicial está pasándose de 16 MiB — probablemente el DOM tiene imágenes inline gigantes; considera servirlas por URL.
- En `sessionStorage` debe existir la clave `faro:session_id`. Si no, el navegador está bloqueando storage (modo privado, Safari ITP). El SDK cae a un id efímero pero la persistencia entre F5 se pierde.

**La sesión existe pero el player dice "No hay eventos para esta sesión".**

- Pasaron más de 7 días — cayó del TTL.
- El primer flush se perdió porque el tab se cerró antes de los 5s iniciales y `sendBeacon` falló (raro pero posible). No hay forma de recuperarlo.

**El replay muestra texto enmascarado donde no debería.**

- Por defecto **todos los inputs se enmascaran**. Si quieres preservar un input concreto, hoy hay que pasar overrides a rrweb directamente — no está expuesto por el SDK aún. Abre un issue si lo necesitas.

**El bundle del cliente engordó mucho.**

- rrweb pesa ~80 KB gzip. Llega vía dynamic `import()` así que solo se descarga cuando `captureSessionReplay: true` **y** la sesión cae dentro del sampling. Si igual te molesta, baja `sessionReplaySampleRate` — los usuarios no muestreados ni siquiera fetchean el chunk.

---

## Cómo lo ves en el dashboard

1. Abres `/errors/<fingerprint>`.
2. La sección **"Sesiones afectadas"** lista hasta 10 sesiones que dispararon ese error. Cada row tiene timestamp, service, session id truncado y un botón **▶ Ver replay** si la sesión está grabada.
3. Cada evento individual del error también muestra inline el `session.id` y un link al replay cuando aplica.
4. El player vive en `/replays/<session_id>` con controles de play/pausa/scrub estándar de `rrweb-player`.

---

## Cambios en el contrato

- v0.4.0 (este release): añade `captureSessionReplay`, `sessionReplaySampleRate`, helper `getSessionId()`. `attributes['session.id']` se incluye automáticamente en todos los eventos.
- Anteriores: ver [README.md](./README.md#changelog).

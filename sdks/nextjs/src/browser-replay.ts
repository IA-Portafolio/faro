/**
 * Session replay (rrweb) — captura el DOM como secuencia de eventos serializables
 * y los envía en chunks al backend para reproducirlos después.
 *
 * Es opt-in (`captureSessionReplay: true`). rrweb se carga dinámicamente; si el
 * consumidor no lo tiene instalado, se loggea un warning y la captura queda
 * desactivada — el resto del SDK (logs, vitals, errors) sigue funcionando.
 *
 * Privacidad por defecto:
 *  - `maskAllInputs: true` enmascara texto de inputs (passwords, etc.)
 *  - `maskTextSelector: '.faro-mask'` enmascara cualquier nodo con esa clase
 *  - `recordCanvas: false` para no exfiltrar canvases (suelen contener imágenes)
 *
 * Los chunks pegan en `/api/v1/ingest/replay` cada `flushIntervalMs` (default 5s)
 * o cuando llegan a `maxEventsPerChunk` (default 80).
 */

export interface SessionReplayOptions {
  endpoint: string;
  token: string;
  service: string;
  sessionId: string;
  /** % de sesiones a grabar. 1.0 = todas. (Default 1.0) */
  sampleRate?: number;
  /** Cadencia de flush en ms (default 5000) */
  flushIntervalMs?: number;
  /** Tamaño máximo de eventos por POST (default 80) */
  maxEventsPerChunk?: number;
  /** Cola en memoria máxima de eventos pendientes (default 1000) */
  maxQueueSize?: number;
  /** user.id que adjuntar al chunk, si lo hay */
  getUserId?: () => string | undefined;
  /**
   * Si true (default), `page_url` se publica sin querystring ni fragmento
   * para no filtrar tokens/PII en query params. Mirroreado desde
   * `FaroBrowserOptions.scrubUrlQuery`.
   */
  scrubUrlQuery?: boolean;
  /**
   * Si true (DEFAULT), enmascara TODO el texto renderizado del DOM en el replay,
   * no sólo los inputs. Sin esto, el replay captura verbatim emails, nombres,
   * saldos, números de cuenta y cualquier PII visible en pantalla y la persiste
   * en el backend. Privacidad primero: ponelo explícitamente en `false` sólo si
   * necesitás replays con texto legible y ya controlás la PII con `.faro-mask`
   * o `blockSelector`. Alinea la superficie de privacidad del replay con la de
   * logs/events (que ya pasan por `scrub*`/`beforeSend`).
   */
  maskAllText?: boolean;
  /** Clase CSS adicional cuyos nodos de texto se enmascaran (además de `.faro-mask`). */
  maskTextClass?: string;
  /** Clase CSS cuyos nodos se EXCLUYEN por completo de la grabación (no se capturan). */
  blockClass?: string;
  /** Selector CSS cuyos nodos se excluyen por completo de la grabación. */
  blockSelector?: string;
  /**
   * Hook para inspeccionar / redactar / descartar cada evento rrweb ANTES de
   * encolarlo. Devolvé el evento (posiblemente modificado) para conservarlo, o
   * `null`/`undefined` para descartarlo. Es el equivalente de `beforeSend` para
   * el replay: permite que la app consumidora filtre PII que el masking de DOM
   * no cubra. Si el hook lanza, se conserva el evento original.
   */
  beforeEmit?: (event: unknown) => unknown | null | undefined;
}

/**
 * Subset estructural de `rrweb.record`. Evita un import tipado de rrweb al
 * compilar el SDK — el consumidor lo instala como peerDep opcional. Si lo tiene,
 * pasa los runtime types al runtime; si no, el dynamic import falla y la captura
 * queda apagada en `initSessionReplay()`.
 */
type RrwebRecord = (opts: {
  emit: (event: unknown, isCheckout?: boolean) => void;
  maskAllInputs?: boolean;
  maskAllText?: boolean;
  maskTextSelector?: string;
  maskTextClass?: string;
  blockClass?: string;
  blockSelector?: string;
  recordCanvas?: boolean;
  checkoutEveryNms?: number;
  sampling?: { mousemove?: number | boolean };
}) => (() => void) | undefined;

interface RrwebModule { record: RrwebRecord }

export interface SessionReplayController {
  /** Detiene la grabación y descarga el último chunk. */
  stop(): void;
  /** Fuerza un flush inmediato. */
  flush(useBeacon?: boolean): Promise<void>;
  /** Si la captura está activa (no descartada por sampling ni por falta de rrweb). */
  readonly active: boolean;
}

/**
 * Devuelve un id de sesión persistente dentro del tab (sobrevive a F5,
 * desaparece al cerrar el tab). Cae a un id efímero en memoria si
 * sessionStorage no está disponible.
 */
export function getOrCreateSessionId(): string {
  const KEY = 'faro:session_id';
  if (typeof sessionStorage !== 'undefined') {
    try {
      const existing = sessionStorage.getItem(KEY);
      if (existing) return existing;
      const fresh = generateId();
      sessionStorage.setItem(KEY, fresh);
      return fresh;
    } catch {
      // sessionStorage bloqueado (Safari ITP / modo privado). Cae al fallback.
    }
  }
  return generateId();
}

function generateId(): string {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return crypto.randomUUID();
    }
  } catch {
    // Continúa al fallback.
  }
  // Fallback: time-prefixed random. No es cripto-fuerte, pero alcanza para
  // distinguir sesiones humanas en un mismo proyecto.
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function initSessionReplay(opts: SessionReplayOptions): SessionReplayController {
  const sampleRate = opts.sampleRate ?? 1.0;
  if (Math.random() > sampleRate) {
    return inactiveController();
  }
  if (typeof window === 'undefined' || typeof document === 'undefined') {
    return inactiveController();
  }

  const endpoint = opts.endpoint.replace(/\/$/, '');
  const url = `${endpoint}/api/v1/ingest/replay`;
  const flushIntervalMs = opts.flushIntervalMs ?? 5000;
  const maxEventsPerChunk = opts.maxEventsPerChunk ?? 80;
  const maxQueueSize = opts.maxQueueSize ?? 1000;
  const scrubUrlQuery = opts.scrubUrlQuery ?? true;
  const pageUrl = (): string => {
    if (typeof window === 'undefined') return '';
    const { location } = window;
    if (!scrubUrlQuery) return location.href;
    return `${location.origin}${location.pathname}`;
  };

  let buffer: unknown[] = [];
  let seq = 0;
  let stopRrweb: (() => void) | undefined;
  let timer: ReturnType<typeof setInterval> | null = null;
  let cleanup: Array<() => void> = [];
  let active = false;
  let stopping = false;

  const flush = async (): Promise<void> => {
    if (buffer.length === 0) return;
    const events = buffer.splice(0, maxEventsPerChunk);
    const chunk = {
      session_id: opts.sessionId,
      service: opts.service,
      seq: seq++,
      events,
      user_id: opts.getUserId?.() ?? '',
      page_url: pageUrl(),
      user_agent: navigator.userAgent,
    };
    const body = JSON.stringify(chunk);

    // Siempre `fetch` con keepalive: funciona durante visibilitychange/pagehide
    // (igual que sendBeacon) PERO soporta el header Authorization. Antes el path
    // de cierre usaba `sendBeacon(url?_token=...)`, que filtra el token de
    // ingesta a access-logs, history del browser y Referer hacia terceros. El
    // hardening ya había sacado el token de la URL en logs/events; el replay era
    // el único `?_token=` que quedaba.
    try {
      const res = await fetch(url, {
        method: 'POST',
        keepalive: true,
        headers: {
          Authorization: `Bearer ${opts.token}`,
          'Content-Type': 'application/json',
        },
        body,
      });
      if (!res.ok && res.status >= 500) {
        // Reintroduce los eventos al principio si fue un fallo de servidor.
        // El seq que ya consumimos queda gastado, pero el backend solo lo usa
        // para ordenamiento — no hay constraint de unicidad.
        buffer.unshift(...events);
      }
    } catch {
      buffer.unshift(...events);
    }
  };

  const pushEvent = (rawEvent: unknown): void => {
    if (stopping) return;
    let event = rawEvent;
    if (opts.beforeEmit) {
      try {
        const out = opts.beforeEmit(rawEvent);
        if (out == null) return; // el consumidor descartó este evento
        event = out;
      } catch {
        // si el hook lanza, conservamos el evento original (no perdemos el replay)
      }
    }
    if (buffer.length >= maxQueueSize) {
      // Antes que crecer sin límite, descartamos los más viejos. Replay degradado
      // > replay que tira un OOM del browser.
      buffer.splice(0, buffer.length - maxQueueSize + 1);
    }
    buffer.push(event);
    if (buffer.length >= maxEventsPerChunk) {
      void flush();
    }
  };

  // Carga rrweb perezosa. Si no está instalado, log y desistir.
  void (async () => {
    let mod: RrwebModule;
    try {
      // rrweb es opcional — no exigimos sus types al compilar el SDK. El nombre
      // del módulo va por una variable para que TS no lo resuelva en tiempo
      // de compilación; el bundler lo deja como dynamic import en runtime.
      const moduleName = 'rrweb';
      mod = (await import(/* @vite-ignore */ /* webpackIgnore: true */ moduleName)) as unknown as RrwebModule;
    } catch (e) {
      console.warn(
        '[faro] captureSessionReplay habilitado pero rrweb no se pudo cargar — ' +
        '`npm install rrweb` en el proyecto consumidor. Replay desactivado.',
        e
      );
      return;
    }
    if (typeof mod.record !== 'function') {
      console.warn('[faro] rrweb cargado pero `record` no es una función');
      return;
    }
    stopRrweb = mod.record({
      emit: pushEvent,
      maskAllInputs: true,
      // Privacidad primero: enmascara TODO el texto renderizado del DOM por
      // defecto (no sólo inputs). El consumidor puede bajarlo con
      // `maskAllText: false` si controla la PII por otros medios.
      maskAllText: opts.maskAllText ?? true,
      maskTextSelector: '.faro-mask',
      maskTextClass: opts.maskTextClass,
      blockClass: opts.blockClass,
      blockSelector: opts.blockSelector,
      recordCanvas: false,
      // Snapshot completo cada 60s — limita el costo de recuperar una sesión
      // a partir de cualquier punto del medio.
      checkoutEveryNms: 60_000,
      // Throttle de mousemove a 50 puntos/segundo, suficiente para reproducir
      // movimientos sin saturar el wire.
      sampling: { mousemove: 50 },
    });
    active = true;

    timer = setInterval(() => void flush(), flushIntervalMs);

    const onHide = (): void => {
      if (document.visibilityState === 'hidden') void flush();
    };
    const onPageHide = (): void => void flush();
    document.addEventListener('visibilitychange', onHide);
    window.addEventListener('pagehide', onPageHide);
    cleanup.push(() => document.removeEventListener('visibilitychange', onHide));
    cleanup.push(() => window.removeEventListener('pagehide', onPageHide));
  })();

  return {
    get active() { return active; },
    async flush(_useBeacon = false) { await flush(); },
    stop() {
      if (stopping) return;
      stopping = true;
      if (timer) clearInterval(timer);
      timer = null;
      if (stopRrweb) {
        try { stopRrweb(); } catch { /* ignora */ }
      }
      for (const fn of cleanup) fn();
      cleanup = [];
      void flush();
    },
  };
}

function inactiveController(): SessionReplayController {
  return {
    get active() { return false; },
    async flush() { /* no-op */ },
    stop() { /* no-op */ },
  };
}

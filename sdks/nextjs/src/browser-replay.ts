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
  maskTextSelector?: string;
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

  const flush = async (useBeacon = false): Promise<void> => {
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

    if (useBeacon && typeof navigator !== 'undefined' && typeof navigator.sendBeacon === 'function') {
      // sendBeacon no permite headers personalizados — token va en query string.
      const beaconUrl = `${url}?_token=${encodeURIComponent(opts.token)}`;
      const ok = navigator.sendBeacon(beaconUrl, new Blob([body], { type: 'application/json' }));
      if (ok) return;
      // Si falla beacon, intenta fetch keepalive abajo.
    }

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

  const pushEvent = (event: unknown): void => {
    if (stopping) return;
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
      maskTextSelector: '.faro-mask',
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
      if (document.visibilityState === 'hidden') void flush(true);
    };
    const onPageHide = (): void => void flush(true);
    document.addEventListener('visibilitychange', onHide);
    window.addEventListener('pagehide', onPageHide);
    cleanup.push(() => document.removeEventListener('visibilitychange', onHide));
    cleanup.push(() => window.removeEventListener('pagehide', onPageHide));
  })();

  return {
    get active() { return active; },
    async flush(useBeacon = false) { await flush(useBeacon); },
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
      void flush(true);
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

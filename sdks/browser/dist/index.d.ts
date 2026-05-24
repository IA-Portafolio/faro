/**
 * @iaportafolio/browser
 *
 * SDK browser para Faro. Captura errores no manejados, Web Vitals, navegaciones
 * y clicks como breadcrumbs, y envía todo en lotes a la API de ingesta usando
 * sendBeacon cuando el tab se cierra (sin perder eventos).
 *
 * Uso mínimo:
 *
 *   import { init } from '@iaportafolio/browser';
 *
 *   init({
 *     endpoint: 'https://faro.iaportafolio.com',
 *     token:    'tu-token-de-proyecto',
 *     service:  'mi-app-web',
 *   });
 *
 *   // a partir de aquí, errores no atrapados se reportan solos
 */
type Severity = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR' | 'FATAL';
interface FaroBrowserOptions {
    endpoint: string;
    token: string;
    service: string;
    environment?: string;
    release?: string;
    /** Atributos por defecto adjuntados a cada evento */
    attributes?: Record<string, string | number | boolean>;
    /** Cadencia de flush en ms (default 2000) */
    flushIntervalMs?: number;
    /** Tamaño máximo de batch por POST (default 100) */
    maxBatchSize?: number;
    /** Cola en memoria máxima (default 2000) */
    maxQueueSize?: number;
    /** Tamaño del ring buffer de breadcrumbs (default 30) */
    maxBreadcrumbs?: number;
    /** Capturar window.onerror + unhandledrejection (default true) */
    captureUnhandled?: boolean;
    /** Capturar console.error y console.warn (default false — puede meter ruido) */
    captureConsole?: boolean;
    /** Capturar Web Vitals LCP/CLS/INP/FID/TTFB (default true) */
    captureWebVitals?: boolean;
    /** Capturar clicks como breadcrumbs (default true) */
    captureClicks?: boolean;
    /** Capturar navegaciones SPA (history.pushState/popstate) (default true) */
    captureNavigation?: boolean;
    /** Hook para muestrear o redactar eventos antes de enviar; devolver null descarta */
    beforeSend?: (event: WireEvent) => WireEvent | null;
}
interface UserContext {
    id?: string;
    email?: string;
    username?: string;
    [key: string]: string | undefined;
}
interface Breadcrumb {
    category: 'click' | 'navigation' | 'console' | 'fetch' | 'custom';
    message: string;
    timestamp: number;
    data?: Record<string, string | number | boolean | undefined>;
}
interface LogEntry {
    level?: Severity;
    message: string;
    attributes?: Record<string, unknown>;
    trace_id?: string;
    span_id?: string;
}
interface WireEvent {
    level: Severity;
    message: string;
    timestamp: string;
    attributes: Record<string, string>;
    trace_id?: string;
    span_id?: string;
}
declare class FaroBrowser {
    private opts;
    private queue;
    private breadcrumbs;
    private user;
    private timer;
    private cleanup;
    private closed;
    constructor(opts: FaroBrowserOptions);
    setUser(user: UserContext | null): void;
    addBreadcrumb(crumb: Omit<Breadcrumb, 'timestamp'>): void;
    log(entry: LogEntry): void;
    info(message: string, attrs?: Record<string, unknown>): void;
    warn(message: string, attrs?: Record<string, unknown>): void;
    error(message: string, attrs?: Record<string, unknown>): void;
    captureException(err: unknown, ctx?: {
        tags?: Record<string, string>;
        message?: string;
    }): void;
    flush(useBeacon?: boolean): Promise<void>;
    close(): void;
    private enqueue;
    private composeAttributes;
    private installErrorHandlers;
    private installConsoleCapture;
    private installWebVitals;
    private installClickTracking;
    private installNavigationTracking;
    private installLifecycleHooks;
}
declare function init(opts: FaroBrowserOptions): FaroBrowser;
declare function getClient(): FaroBrowser;
declare function log(entry: LogEntry): void;
declare function info(msg: string, attrs?: Record<string, unknown>): void;
declare function warn(msg: string, attrs?: Record<string, unknown>): void;
declare function error(msg: string, attrs?: Record<string, unknown>): void;
declare function captureException(err: unknown, ctx?: {
    tags?: Record<string, string>;
    message?: string;
}): void;
declare function setUser(user: UserContext | null): void;
declare function addBreadcrumb(crumb: Omit<Breadcrumb, 'timestamp'>): void;
declare function flush(): Promise<void>;
declare function close(): void;

export { type Breadcrumb, FaroBrowser, type FaroBrowserOptions, type LogEntry, type Severity, type UserContext, type WireEvent, addBreadcrumb, captureException, close, error, flush, getClient, info, init, log, setUser, warn };

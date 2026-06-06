/**
 * Catálogo unificado de comandos para la paleta global (⌘K).
 *
 * Hay tres fuentes:
 *   - **Estáticos**: navegación + cambios de tema. Conocidos al compilar.
 *   - **Entidades**: proyectos, servicios, monitores y reglas de alerta. Se
 *     cargan al abrir la paleta y se cachean en memoria con TTL.
 *   - **Saltos directos**: respuesta a sintaxis tipo `traces:<id>`,
 *     `logs:trace=<id>`, `errors:<fp>`. Se generan en función de la query
 *     escrita por el usuario.
 */

import { get } from 'svelte/store';
import { goto } from '$app/navigation';

import {
  fetchAlertRules,
  fetchMonitors,
  fetchProjects,
  fetchServices,
  type AlertRule,
  type Monitor,
  type Project,
  type Service
} from './api';
import { selectedProject } from './stores';
import { setTheme, type ThemeChoice } from './theme';

export type CommandGroup =
  | 'Salto directo'
  | 'Navegar'
  | 'Proyectos'
  | 'Servicios'
  | 'Monitores'
  | 'Alertas'
  | 'Tema';

export type Command = {
  id: string;
  group: CommandGroup;
  label: string;
  /** Texto secundario gris (slug, descripción corta). */
  sub?: string;
  /** Texto adicional usado para el filtro fuzzy (no se renderiza). */
  keywords?: string;
  /** Atajo asociado, si lo hay. */
  shortcut?: string;
  /** Icono ascii a la izquierda. */
  icon?: string;
  /** Etiqueta a la derecha (badge contextual). */
  hint?: string;
  run: () => void | Promise<void>;
};

// ---------- Comandos estáticos ----------

export function staticCommands(): Command[] {
  return [
    { id: 'nav.home',     group: 'Navegar', icon: '◐', label: 'Ir a Resumen',       shortcut: 'g r', run: () => goto('/') },
    { id: 'nav.logs',     group: 'Navegar', icon: '≡', label: 'Ir a Logs',          shortcut: 'g l', run: () => goto('/logs') },
    { id: 'nav.traces',   group: 'Navegar', icon: '⤳', label: 'Ir a Trazas',        shortcut: 'g t', run: () => goto('/traces') },
    { id: 'nav.metrics',  group: 'Navegar', icon: '◢', label: 'Ir a Métricas',      shortcut: 'g m', run: () => goto('/metrics') },
    { id: 'nav.errors',   group: 'Navegar', icon: '⚠', label: 'Ir a Errores',       shortcut: 'g e', run: () => goto('/errors') },
    { id: 'nav.insights', group: 'Navegar', icon: '◈', label: 'Ir a Insights',       run: () => goto('/insights') },
    { id: 'nav.monitors', group: 'Navegar', icon: '◉', label: 'Ir a Monitores',     shortcut: 'g o', run: () => goto('/monitors') },
    { id: 'nav.docs',     group: 'Navegar', icon: '⌗', label: 'Ir a SDKs & API',    run: () => goto('/docs') },
    { id: 'nav.settings', group: 'Navegar', icon: '⚙', label: 'Ir a Configuración', shortcut: 'g s', run: () => goto('/settings') },
    { id: 'nav.alerts',   group: 'Navegar', icon: '⏰', label: 'Ir a Alertas',       shortcut: 'g a', run: () => goto('/settings/alerts') },
    { id: 'nav.projects', group: 'Navegar', icon: '⚙', label: 'Ir a Proyectos',     shortcut: 'g p', run: () => goto('/settings/projects') },
    { id: 'nav.product-users', group: 'Navegar', icon: '◌', label: 'Ir a Usuarios de producto', shortcut: 'g u', run: () => goto('/users') },
    { id: 'nav.sessions', group: 'Navegar', icon: '▤', label: 'Ir a Sesiones', run: () => goto('/sessions') },
    { id: 'nav.retention', group: 'Navegar', icon: '▦', label: 'Ir a Retention', shortcut: 'g n', run: () => goto('/retention') },
    { id: 'nav.settings-users', group: 'Navegar', icon: '◍', label: 'Ir a Usuarios del dashboard', run: () => goto('/settings/users') },
    { id: 'nav.integ',    group: 'Navegar', icon: '⇆', label: 'Ir a Integraciones', shortcut: 'g i', run: () => goto('/settings/integrations') },
    { id: 'nav.appearance', group: 'Navegar', icon: '◐', label: 'Ir a Apariencia',  run: () => goto('/settings/appearance') },

    { id: 'theme.light',  group: 'Tema', icon: '☀', label: 'Cambiar a tema claro',   run: () => setTheme('light') },
    { id: 'theme.dark',   group: 'Tema', icon: '☾', label: 'Cambiar a tema oscuro',  run: () => setTheme('dark') },
    { id: 'theme.system', group: 'Tema', icon: '◐', label: 'Seguir tema del sistema', run: () => setTheme('system' as ThemeChoice) }
  ];
}

// ---------- Caché de entidades ----------

type EntityCache = {
  fetchedAt: number;
  projectKey: string;
  commands: Command[];
};

const TTL_MS = 60_000;
let cache: EntityCache | null = null;

/** Invalida el caché — útil al crear/borrar entidades o al cambiar de proyecto. */
export function invalidatePaletteCache(): void {
  cache = null;
}

export async function loadEntityCommands(force = false): Promise<Command[]> {
  const projectKey = get(selectedProject) || '';
  const now = Date.now();
  if (
    !force &&
    cache &&
    cache.projectKey === projectKey &&
    now - cache.fetchedAt < TTL_MS
  ) {
    return cache.commands;
  }

  // Lanza en paralelo y tolera fallos individuales para que un endpoint
  // caído no rompa la paleta entera.
  const [projects, services, monitors, rules] = await Promise.all([
    fetchProjects().catch(() => [] as Project[]),
    fetchServices({ last_minutes: 1440, project: projectKey || undefined })
      .catch(() => [] as Service[]),
    fetchMonitors().catch(() => [] as Monitor[]),
    fetchAlertRules().catch(() => [] as AlertRule[])
  ]);

  const commands: Command[] = [];

  for (const p of projects) {
    const isActive = p.slug === projectKey;
    commands.push({
      id: `project.${p.slug}`,
      group: 'Proyectos',
      icon: '⊟',
      label: `Filtrar por proyecto: ${p.name}`,
      sub: p.slug,
      keywords: `${p.slug} ${p.name} ${p.description ?? ''}`,
      hint: isActive ? 'Activo' : undefined,
      run: () => selectedProject.set(p.slug)
    });
  }
  // Atajo extra para "ver todos los proyectos".
  commands.push({
    id: 'project.__all__',
    group: 'Proyectos',
    icon: '∗',
    label: 'Quitar filtro de proyecto (ver todos)',
    keywords: 'todos all clear',
    hint: projectKey ? undefined : 'Activo',
    run: () => selectedProject.set('')
  });

  for (const s of services) {
    commands.push({
      id: `service.logs.${s.service_name}`,
      group: 'Servicios',
      icon: '⌬',
      label: `Logs de ${s.service_name}`,
      sub: `${s.log_count.toLocaleString()} logs · ${s.error_count.toLocaleString()} errores`,
      keywords: `${s.service_name} logs servicio service`,
      run: () => goto(`/logs?service=${encodeURIComponent(s.service_name)}`)
    });
    commands.push({
      id: `service.errors.${s.service_name}`,
      group: 'Servicios',
      icon: '⚠',
      label: `Errores de ${s.service_name}`,
      sub: `${s.error_count.toLocaleString()} errores`,
      keywords: `${s.service_name} errores errors`,
      run: () => goto(`/errors?service=${encodeURIComponent(s.service_name)}`)
    });
  }

  for (const m of monitors) {
    commands.push({
      id: `monitor.${m.id}`,
      group: 'Monitores',
      icon: '◉',
      label: `Monitor: ${m.name}`,
      sub: `${m.method} ${m.url}`,
      keywords: `${m.name} ${m.url} ${m.method} monitor uptime`,
      hint: m.enabled ? undefined : 'OFF',
      // No hay página de detalle todavía; al menos lleva al listado con
      // un anchor por id para que se pueda hacer scroll-into-view en el futuro.
      run: () => goto(`/monitors#monitor-${m.id}`)
    });
  }

  for (const r of rules) {
    commands.push({
      id: `alert.${r.id}`,
      group: 'Alertas',
      icon: '⏰',
      label: `Regla: ${r.name}`,
      sub: r.description || `${r.source} · ${r.condition} ${r.threshold}`,
      keywords: `${r.name} ${r.description} ${r.source} alerta alert rule`,
      hint: r.enabled ? r.severity : 'OFF',
      run: () => goto(`/settings/alerts#rule-${r.id}`)
    });
  }

  cache = { fetchedAt: now, projectKey, commands };
  return commands;
}

// ---------- Saltos directos por sintaxis ----------

/** Sanea un identificador hex (trace id, span id, fingerprint). */
function cleanId(s: string): string {
  return s.trim().replace(/^["']|["']$/g, '');
}

/**
 * Examina la query del usuario en busca de prefijos especiales y devuelve
 * comandos "salto directo" ordenados al inicio del resultado.
 *
 * Acepta variantes razonables:
 *   - `traces:abc123`  o  `trace:abc123`
 *   - `logs:trace=abc123`  o  `logs:abc123`  (asume trace_id)
 *   - `errors:fp`  o  `error:fp`  o  `issue:fp`
 */
export function jumpCommands(query: string): Command[] {
  const q = query.trim();
  if (!q) return [];
  const out: Command[] = [];

  const traceMatch = q.match(/^(?:traces?|tr)\s*[:=\s]\s*(\S+)/i);
  if (traceMatch) {
    const id = cleanId(traceMatch[1]);
    if (id) {
      out.push({
        id: `jump.trace.${id}`,
        group: 'Salto directo',
        icon: '⤳',
        label: `Abrir traza ${id}`,
        sub: '/traces/' + id,
        hint: 'Enter',
        run: () => goto(`/traces/${encodeURIComponent(id)}`)
      });
      out.push({
        id: `jump.tracelogs.${id}`,
        group: 'Salto directo',
        icon: '≡',
        label: `Ver logs de la traza ${id}`,
        sub: `/logs?trace_id=${id}`,
        run: () => goto(`/logs?trace_id=${encodeURIComponent(id)}`)
      });
    }
  }

  const logsMatch = q.match(/^logs?\s*[:=\s]\s*(?:trace\s*[=:]\s*)?(\S+)/i);
  if (logsMatch && !traceMatch) {
    // Sintaxis `logs:trace=xyz` o `logs:xyz` (asumido trace_id).
    const raw = cleanId(logsMatch[1]);
    if (raw) {
      out.push({
        id: `jump.logs.trace.${raw}`,
        group: 'Salto directo',
        icon: '≡',
        label: `Ver logs de la traza ${raw}`,
        sub: `/logs?trace_id=${raw}`,
        hint: 'Enter',
        run: () => goto(`/logs?trace_id=${encodeURIComponent(raw)}`)
      });
    }
  }

  const errMatch = q.match(/^(?:errors?|issue|fp)\s*[:=\s]\s*(\S+)/i);
  if (errMatch) {
    const fp = cleanId(errMatch[1]);
    if (fp) {
      out.push({
        id: `jump.error.${fp}`,
        group: 'Salto directo',
        icon: '⚠',
        label: `Abrir issue ${fp}`,
        sub: `/errors/${fp}`,
        hint: 'Enter',
        run: () => goto(`/errors/${encodeURIComponent(fp)}`)
      });
    }
  }

  return out;
}

// ---------- Fuzzy match ----------

function tokens(s: string): string[] {
  return s.toLowerCase().split(/\s+/).filter(Boolean);
}

export function matches(c: Command, q: string): boolean {
  if (!q) return true;
  const haystack = `${c.label} ${c.sub ?? ''} ${c.group} ${c.keywords ?? ''} ${c.shortcut ?? ''}`.toLowerCase();
  return tokens(q).every((t) => haystack.includes(t));
}

// ---------- Scoring ----------

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

// Cuanto mayor el score, más relevante. 0 = no matchea (excluido por search).
function scoreToken(c: Command, t: string): number {
  const label = c.label.toLowerCase();
  const sub = (c.sub ?? '').toLowerCase();
  const kw = (c.keywords ?? '').toLowerCase();
  const sc = (c.shortcut ?? '').toLowerCase();
  const grp = c.group.toLowerCase();
  let best = 0;
  // Palabra completa en label (entre límites \b) — la señal más fuerte.
  if (new RegExp(`\\b${escapeRegex(t)}\\b`).test(label)) best = Math.max(best, 100);
  // Label arranca con el token.
  if (label.startsWith(t)) best = Math.max(best, 80);
  // Substring en label.
  if (label.includes(t)) best = Math.max(best, 60);
  // Otros campos, en orden decreciente de "intención del usuario".
  if (sub.includes(t)) best = Math.max(best, 30);
  if (kw.includes(t)) best = Math.max(best, 25);
  if (sc.includes(t)) best = Math.max(best, 15);
  if (grp.includes(t)) best = Math.max(best, 5);
  return best;
}

/**
 * Suma de scores por token (modelo AND: todos los tokens deben matchear).
 * Devuelve -Infinity si algún token no matchea ⇒ el comando queda fuera de
 * search(). Saltos directos reciben un bonus enorme: el parser de
 * `jumpCommands` ya garantizó que son relevantes para esta query.
 */
export function score(c: Command, q: string): number {
  if (c.group === 'Salto directo') return 1000;
  const ts = tokens(q);
  if (ts.length === 0) return 0;
  let total = 0;
  for (const t of ts) {
    const s = scoreToken(c, t);
    if (s === 0) return -Infinity;
    total += s;
  }
  return total;
}

/**
 * Filtra + ordena por score descendente. Determinístico:
 *   1. score mayor primero
 *   2. label más corto primero (más específico)
 *   3. id lexicográfico ascendente
 *   4. orden de entrada (estable)
 *
 * Con query vacía, devuelve la lista intacta (sin reordenar).
 */
export function search(commands: Command[], query: string): Command[] {
  const q = query.trim();
  if (!q) return commands.slice();
  const scored: Array<{ c: Command; s: number; i: number }> = [];
  for (let i = 0; i < commands.length; i++) {
    const c = commands[i];
    const s = score(c, q);
    if (s > -Infinity) scored.push({ c, s, i });
  }
  scored.sort((a, b) => {
    if (b.s !== a.s) return b.s - a.s;
    if (a.c.label.length !== b.c.label.length) return a.c.label.length - b.c.label.length;
    if (a.c.id !== b.c.id) return a.c.id < b.c.id ? -1 : 1;
    return a.i - b.i;
  });
  return scored.map((x) => x.c);
}

// ---------- Navegación con flechas ----------

/**
 * Calcula el siguiente índice resaltado al pulsar flechas (sin wrap-around).
 * `dir = 1` para ↓, `dir = -1` para ↑. Si la lista está vacía, devuelve 0.
 */
export function nextHighlight(current: number, length: number, dir: -1 | 1): number {
  if (length <= 0) return 0;
  const next = current + dir;
  if (next < 0) return 0;
  if (next >= length) return length - 1;
  return next;
}

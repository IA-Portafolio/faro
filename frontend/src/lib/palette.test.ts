import { describe, it, expect } from 'vitest';
import {
  jumpCommands,
  matches,
  nextHighlight,
  score,
  search,
  staticCommands,
  type Command,
  type CommandGroup
} from './palette';

// ---------- Helpers ----------

// Builder mínimo para no repetir `run: () => {}` en cada caso.
function cmd(overrides: Partial<Command> & Pick<Command, 'id' | 'group' | 'label'>): Command {
  return {
    run: () => {},
    ...overrides
  };
}

// Fixture representativo: navegación estática + un par de "Logs de <servicio>"
// para verificar `search('logs api')`.
function sampleCommands(): Command[] {
  return [
    cmd({ id: 'nav.logs', group: 'Navegar', label: 'Ir a Logs', shortcut: 'g l' }),
    cmd({ id: 'nav.traces', group: 'Navegar', label: 'Ir a Trazas', shortcut: 'g t' }),
    cmd({ id: 'nav.errors', group: 'Navegar', label: 'Ir a Errores', shortcut: 'g e' }),
    cmd({
      id: 'service.logs.api',
      group: 'Servicios',
      label: 'Logs de api',
      sub: '1.000 logs · 5 errores',
      keywords: 'api logs servicio service'
    }),
    cmd({
      id: 'service.logs.web',
      group: 'Servicios',
      label: 'Logs de web',
      sub: '500 logs · 2 errores',
      keywords: 'web logs servicio service'
    }),
    cmd({
      id: 'service.errors.api',
      group: 'Servicios',
      label: 'Errores de api',
      sub: '5 errores',
      keywords: 'api errores errors'
    }),
    cmd({
      id: 'project.api-gateway',
      group: 'Proyectos',
      label: 'Filtrar por proyecto: api-gateway',
      sub: 'api-gateway',
      keywords: 'api-gateway api gateway'
    })
  ];
}

// ---------- matches() (boolean filter, mantiene compat con el comportamiento previo) ----------

describe('matches', () => {
  const c = cmd({
    id: 'x',
    group: 'Servicios' satisfies CommandGroup,
    label: 'Logs de api',
    sub: '1.000 logs',
    keywords: 'api servicio',
    shortcut: 'g l'
  });

  it('query vacía siempre matchea', () => {
    expect(matches(c, '')).toBe(true);
    expect(matches(c, '   ')).toBe(true);
  });

  it('matchea substring en label', () => {
    expect(matches(c, 'logs')).toBe(true);
    expect(matches(c, 'api')).toBe(true);
  });

  it('es case-insensitive', () => {
    expect(matches(c, 'LOGS')).toBe(true);
    expect(matches(c, 'ApI')).toBe(true);
  });

  it('AND entre tokens — todos deben matchear', () => {
    expect(matches(c, 'logs api')).toBe(true);
    expect(matches(c, 'logs noexiste')).toBe(false);
  });

  it('matchea en sub, keywords y shortcut', () => {
    expect(matches(c, '1.000')).toBe(true);
    expect(matches(c, 'servicio')).toBe(true);
    expect(matches(c, 'g l')).toBe(true);
  });
});

// ---------- score() ----------

describe('score', () => {
  it('query vacía ⇒ score 0 (no se ordena nada)', () => {
    const c = cmd({ id: 'x', group: 'Navegar', label: 'Ir a Logs' });
    expect(score(c, '')).toBe(0);
    expect(score(c, '   ')).toBe(0);
  });

  it('palabra completa en label puntúa más que substring', () => {
    const word = cmd({ id: 'a', group: 'Navegar', label: 'Ir a Logs' });
    const sub = cmd({ id: 'b', group: 'Navegar', label: 'Sin-prefijo-logsmore-cosas' });
    expect(score(word, 'logs')).toBeGreaterThan(score(sub, 'logs'));
  });

  it('prefijo de label puntúa más que substring no-prefijo', () => {
    const pref = cmd({ id: 'a', group: 'Servicios', label: 'apiCore' });
    // "xxapifoo" → no es palabra completa "api" ni prefijo "api", solo substring.
    const subOnly = cmd({ id: 'b', group: 'Servicios', label: 'xxapifoo' });
    expect(score(pref, 'api')).toBeGreaterThan(score(subOnly, 'api'));
  });

  it('match en label > match en sub > match en keywords > match en group', () => {
    const inLabel = cmd({ id: 'a', group: 'Servicios', label: 'foo' });
    const inSub = cmd({ id: 'b', group: 'Servicios', label: 'bar', sub: 'foo' });
    const inKw = cmd({ id: 'c', group: 'Servicios', label: 'bar', keywords: 'foo' });
    const inGrp = cmd({ id: 'd', group: 'Servicios', label: 'bar' }); // matchea por 'servicios'
    expect(score(inLabel, 'foo')).toBeGreaterThan(score(inSub, 'foo'));
    expect(score(inSub, 'foo')).toBeGreaterThan(score(inKw, 'foo'));
    expect(score(inKw, 'foo')).toBeGreaterThan(score(inGrp, 'servicios'));
  });

  it('un token sin match en ningún campo ⇒ -Infinity (excluido)', () => {
    const c = cmd({ id: 'x', group: 'Navegar', label: 'Ir a Logs' });
    expect(score(c, 'zzz')).toBe(-Infinity);
    expect(score(c, 'logs zzz')).toBe(-Infinity);
  });

  it('AND entre tokens — todos suman, ninguno falta', () => {
    const c = cmd({
      id: 'x',
      group: 'Servicios',
      label: 'Logs de api',
      keywords: 'servicio'
    });
    expect(score(c, 'logs')).toBeGreaterThan(0);
    expect(score(c, 'api')).toBeGreaterThan(0);
    expect(score(c, 'logs api')).toBeGreaterThan(score(c, 'logs'));
  });

  it('Salto directo siempre ⇒ score muy alto, sin importar la query', () => {
    const jump = cmd({
      id: 'jump.trace.abc',
      group: 'Salto directo',
      label: 'Abrir traza abc'
    });
    // Aunque el token no aparezca en ningún campo, el jump ya fue pre-validado
    // por jumpCommands(query) — debe puntuar arriba de todo y de forma constante.
    expect(score(jump, 'zzz-no-match')).toBeGreaterThan(500);
    expect(score(jump, 'cualquier cosa')).toBeGreaterThan(500);
    expect(score(jump, '')).toBeGreaterThan(500);
    // Score constante: el cortocircuito no depende de la query.
    expect(score(jump, 'zzz-no-match')).toBe(score(jump, ''));
  });

  it('Salto directo gana siempre vs match perfecto en label de otro comando', () => {
    const jump = cmd({ id: 'jump.x', group: 'Salto directo', label: 'Abrir traza abc' });
    const normal = cmd({ id: 'nav.logs', group: 'Navegar', label: 'logs' });
    // Match exacto de palabra completa en label = 100; un jump da 1000.
    expect(score(jump, 'logs')).toBeGreaterThan(score(normal, 'logs'));
  });
});

// ---------- search() ----------

describe('search', () => {
  it('query vacía devuelve la lista intacta (sin reordenar)', () => {
    const list = sampleCommands();
    const out = search(list, '');
    expect(out).toHaveLength(list.length);
    expect(out.map((c) => c.id)).toEqual(list.map((c) => c.id));
  });

  it('devuelve copia (no muta la lista original) con query vacía', () => {
    const list = sampleCommands();
    const out = search(list, '');
    expect(out).not.toBe(list);
  });

  it('search("logs api") ordena los matches con "Logs de api" primero', () => {
    const out = search(sampleCommands(), 'logs api');
    // El comando más obvio para "logs api" es "Logs de api".
    expect(out[0].id).toBe('service.logs.api');
  });

  it('search("logs api") excluye los que no matchean todos los tokens', () => {
    const out = search(sampleCommands(), 'logs api');
    const ids = out.map((c) => c.id);
    // "Ir a Trazas" no tiene ni "logs" ni "api" ⇒ fuera.
    expect(ids).not.toContain('nav.traces');
    // "Logs de web" tiene "logs" pero no "api" ⇒ fuera.
    expect(ids).not.toContain('service.logs.web');
    // "Errores de api" tiene "api" pero no "logs" ⇒ fuera.
    expect(ids).not.toContain('service.errors.api');
    // Sí debe estar el target.
    expect(ids).toContain('service.logs.api');
  });

  it('search es determinístico: misma entrada ⇒ misma salida (ids estables)', () => {
    const list = sampleCommands();
    const a = search(list, 'logs').map((c) => c.id);
    const b = search(list, 'logs').map((c) => c.id);
    expect(a).toEqual(b);
  });

  it('Saltos directos se colocan delante del resto de matches', () => {
    const list: Command[] = [
      ...sampleCommands(),
      cmd({
        id: 'jump.trace.deadbeef',
        group: 'Salto directo',
        label: 'Abrir traza deadbeef',
        sub: '/traces/deadbeef'
      })
    ];
    const out = search(list, 'logs');
    expect(out[0].group).toBe('Salto directo');
  });

  it('desempates: a igual score, gana el label más corto', () => {
    const shortLabel = cmd({ id: 'long.id.zzz', group: 'Servicios', label: 'foo bar' });
    const longLabel = cmd({ id: 'aaa.id', group: 'Servicios', label: 'foo bar baz qux' });
    const out = search([longLabel, shortLabel], 'foo bar');
    expect(out[0].id).toBe('long.id.zzz');
    expect(out[1].id).toBe('aaa.id');
  });

  it('desempates: si label tiene la misma longitud, gana el id lexicográfico menor', () => {
    const a = cmd({ id: 'a.same', group: 'Servicios', label: 'foo bar' });
    const b = cmd({ id: 'b.same', group: 'Servicios', label: 'baz qux' });
    // Ambos tienen label de longitud 7 y match por substring en label/group.
    // Solo "foo" matchea label de a → necesitamos query que matchee ambos al
    // mismo nivel. Usamos 'servicios' (matchea solo group en ambos) ⇒ mismo score.
    const out = search([b, a], 'servicios');
    expect(out[0].id).toBe('a.same');
    expect(out[1].id).toBe('b.same');
  });
});

// ---------- jumpCommands() ----------

describe('jumpCommands', () => {
  it('query vacía ⇒ []', () => {
    expect(jumpCommands('')).toEqual([]);
    expect(jumpCommands('   ')).toEqual([]);
  });

  it('reconoce traces:<id> y emite jump al detalle + jump a logs de esa traza', () => {
    const out = jumpCommands('traces:abc123');
    expect(out).toHaveLength(2);
    expect(out[0].id).toBe('jump.trace.abc123');
    expect(out[0].group).toBe('Salto directo');
    expect(out[1].id).toBe('jump.tracelogs.abc123');
  });

  it('acepta variantes de prefijo de traza (trace, tr, separador = o espacio)', () => {
    expect(jumpCommands('trace:abc')[0]?.id).toBe('jump.trace.abc');
    expect(jumpCommands('tr abc')[0]?.id).toBe('jump.trace.abc');
    expect(jumpCommands('TRACES=abc')[0]?.id).toBe('jump.trace.abc');
  });

  it('logs:<id> emite jump asumiendo trace_id', () => {
    const out = jumpCommands('logs:xyz');
    expect(out).toHaveLength(1);
    expect(out[0].id).toBe('jump.logs.trace.xyz');
  });

  it('logs:trace=<id> también funciona', () => {
    const out = jumpCommands('logs:trace=xyz');
    expect(out[0].id).toBe('jump.logs.trace.xyz');
  });

  it('errors:<fp> e issue:<fp> emiten jump al issue', () => {
    expect(jumpCommands('errors:deadbeef')[0]?.id).toBe('jump.error.deadbeef');
    expect(jumpCommands('issue:deadbeef')[0]?.id).toBe('jump.error.deadbeef');
    expect(jumpCommands('error:deadbeef')[0]?.id).toBe('jump.error.deadbeef');
  });

  it('strip de comillas circundantes en el id', () => {
    expect(jumpCommands('traces:"abc"')[0]?.id).toBe('jump.trace.abc');
    expect(jumpCommands("traces:'abc'")[0]?.id).toBe('jump.trace.abc');
  });

  it('query sin prefijo conocido ⇒ []', () => {
    expect(jumpCommands('logs')).toEqual([]);
    expect(jumpCommands('cualquier cosa libre')).toEqual([]);
  });
});

// ---------- nextHighlight() (navegación con flechas) ----------

describe('nextHighlight', () => {
  it('avanza con dir=1 dentro del rango', () => {
    expect(nextHighlight(0, 5, 1)).toBe(1);
    expect(nextHighlight(3, 5, 1)).toBe(4);
  });

  it('retrocede con dir=-1 dentro del rango', () => {
    expect(nextHighlight(4, 5, -1)).toBe(3);
    expect(nextHighlight(1, 5, -1)).toBe(0);
  });

  it('clamp inferior: dir=-1 desde 0 se queda en 0 (no wrap)', () => {
    expect(nextHighlight(0, 5, -1)).toBe(0);
  });

  it('clamp superior: dir=1 desde el último se queda en el último (no wrap)', () => {
    expect(nextHighlight(4, 5, 1)).toBe(4);
  });

  it('lista vacía ⇒ devuelve 0 sin caer en -1', () => {
    expect(nextHighlight(0, 0, 1)).toBe(0);
    expect(nextHighlight(0, 0, -1)).toBe(0);
    expect(nextHighlight(5, 0, 1)).toBe(0);
  });

  it('lista de 1 ítem ⇒ siempre 0', () => {
    expect(nextHighlight(0, 1, 1)).toBe(0);
    expect(nextHighlight(0, 1, -1)).toBe(0);
  });
});

// ---------- staticCommands() (smoke test: shape estable) ----------

describe('staticCommands', () => {
  it('devuelve un set conocido de comandos con campos obligatorios', () => {
    const cmds = staticCommands();
    expect(cmds.length).toBeGreaterThan(0);
    for (const c of cmds) {
      expect(c.id).toBeTruthy();
      expect(c.group).toBeTruthy();
      expect(c.label).toBeTruthy();
      expect(typeof c.run).toBe('function');
    }
    // Atajos clave que el resto del UI da por hecho.
    const ids = cmds.map((c) => c.id);
    expect(ids).toContain('nav.logs');
    expect(ids).toContain('nav.traces');
    expect(ids).toContain('nav.product-users');
    expect(ids).toContain('nav.settings-users');
    expect(ids).toContain('theme.dark');
  });
});

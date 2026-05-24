import { describe, it, expect, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import {
  isValidRange,
  rangeMinutes,
  formatTimestamp,
  formatDuration,
  severityClass,
  selectedProject,
  timeRange,
  currentUser
} from './stores';

describe('isValidRange', () => {
  it('acepta todos los presets soportados', () => {
    for (const r of ['5m', '15m', '1h', '6h', '24h', '7d']) {
      expect(isValidRange(r)).toBe(true);
    }
  });

  it('rechaza valores fuera del set', () => {
    expect(isValidRange('')).toBe(false);
    expect(isValidRange('30m')).toBe(false);
    expect(isValidRange('1H')).toBe(false); // case-sensitive
    expect(isValidRange('foo')).toBe(false);
  });
});

describe('rangeMinutes', () => {
  it('mapea cada preset al número de minutos correcto', () => {
    expect(rangeMinutes('5m')).toBe(5);
    expect(rangeMinutes('15m')).toBe(15);
    expect(rangeMinutes('1h')).toBe(60);
    expect(rangeMinutes('6h')).toBe(360);
    expect(rangeMinutes('24h')).toBe(1440);
    expect(rangeMinutes('7d')).toBe(10080);
  });
});

describe('formatDuration', () => {
  // Recordatorio: la entrada son NANOSEGUNDOS.
  it('devuelve "0" para falsy', () => {
    expect(formatDuration(0)).toBe('0');
  });

  it('< 1µs ⇒ ns sin decimales', () => {
    expect(formatDuration(1)).toBe('1ns');
    expect(formatDuration(999)).toBe('999ns');
  });

  it('< 1ms ⇒ µs con 1 decimal', () => {
    expect(formatDuration(1000)).toBe('1.0µs');
    expect(formatDuration(1500)).toBe('1.5µs');
    expect(formatDuration(999_999)).toBe('1000.0µs');
  });

  it('< 1s ⇒ ms con 2 decimales', () => {
    expect(formatDuration(1_000_000)).toBe('1.00ms');
    expect(formatDuration(1_500_000)).toBe('1.50ms');
    expect(formatDuration(123_456_789)).toBe('123.46ms');
  });

  it('≥ 1s ⇒ s con 2 decimales', () => {
    expect(formatDuration(1_000_000_000)).toBe('1.00s');
    expect(formatDuration(1_500_000_000)).toBe('1.50s');
    expect(formatDuration(60_000_000_000)).toBe('60.00s');
  });
});

describe('severityClass', () => {
  it('mapea los niveles canónicos', () => {
    expect(severityClass('TRACE')).toBe('trace');
    expect(severityClass('DEBUG')).toBe('debug');
    expect(severityClass('INFO')).toBe('info');
    expect(severityClass('WARN')).toBe('warn');
    expect(severityClass('ERROR')).toBe('error');
    expect(severityClass('FATAL')).toBe('fatal');
  });

  it('es case-insensitive', () => {
    expect(severityClass('trace')).toBe('trace');
    expect(severityClass('Debug')).toBe('debug');
    expect(severityClass('info')).toBe('info');
  });

  it('soporta variantes con sufijo (WARNING, ERROR4, etc.)', () => {
    expect(severityClass('WARNING')).toBe('warn');
    expect(severityClass('ERROR4')).toBe('error');
    expect(severityClass('FATAL2')).toBe('fatal');
  });

  it('alias especiales: ERR y CRIT*', () => {
    expect(severityClass('ERR')).toBe('error');
    expect(severityClass('CRIT')).toBe('fatal');
    expect(severityClass('CRITICAL')).toBe('fatal');
  });

  it('cae a "info" para vacío, null-ish o desconocido', () => {
    expect(severityClass('')).toBe('info');
    // @ts-expect-error: en runtime puede llegar undefined desde el backend
    expect(severityClass(undefined)).toBe('info');
    expect(severityClass('lol-no-existe')).toBe('info');
  });
});

describe('formatTimestamp', () => {
  it('devuelve "" para string vacío', () => {
    expect(formatTimestamp('')).toBe('');
  });

  it('devuelve la entrada cruda si la fecha es inválida', () => {
    expect(formatTimestamp('not-a-date')).toBe('not-a-date');
  });

  it('formatea ISO con T', () => {
    // No hardcodeamos el formato exacto (depende del locale del runner) pero
    // exigimos que aparezcan los componentes esperados: año 2026, mes 05, día 24
    // y los milisegundos del input (123).
    const out = formatTimestamp('2026-05-24T12:34:56.123Z');
    expect(out).toMatch(/2026/);
    expect(out).toMatch(/05/);
    expect(out).toMatch(/24/);
    expect(out).toMatch(/123/);
  });

  it('acepta el formato ClickHouse (espacio en vez de T) y lo trata como UTC', () => {
    // El código reemplaza " " por "T" y agrega "Z" si no había T. Ambos
    // formatos deben producir EXACTAMENTE la misma salida formateada.
    const iso = formatTimestamp('2026-05-24T12:34:56.123Z');
    const ch = formatTimestamp('2026-05-24 12:34:56.123');
    expect(ch).toBe(iso);
  });

  it('soporta timestamps en cualquier offset (ISO con Z)', () => {
    // Mismo instante absoluto expresado con offset distinto: el resultado
    // formateado debe ser el mismo (toLocaleString usa el TZ del runner, pero
    // el INSTANTE es idéntico).
    const a = formatTimestamp('2026-05-24T12:00:00.000Z');
    const b = formatTimestamp('2026-05-24T14:00:00.000+02:00');
    expect(a).toBe(b);
  });
});

// Sanity-check: los writables existen y arrancan en los valores documentados.
// No es el foco del archivo pero confirma que no rompemos el export shape.
describe('writables exportados', () => {
  beforeEach(() => {
    selectedProject.set('');
    timeRange.set('1h');
    currentUser.set(null);
  });

  it('selectedProject arranca vacío y acepta updates', () => {
    expect(get(selectedProject)).toBe('');
    selectedProject.set('faro');
    expect(get(selectedProject)).toBe('faro');
  });

  it('timeRange arranca en "1h" y acepta presets', () => {
    expect(get(timeRange)).toBe('1h');
    timeRange.set('24h');
    expect(get(timeRange)).toBe('24h');
  });

  it('currentUser arranca null', () => {
    expect(get(currentUser)).toBeNull();
  });
});

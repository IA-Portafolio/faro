import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

import {
  dismiss,
  dismissAll,
  pauseDismiss,
  resumeDismiss,
  toast,
  toasts,
  type Toast
} from './toasts';

function current(): Toast[] {
  return get(toasts);
}

beforeEach(() => {
  // Module-level store + timers persist across tests — reset before each one.
  dismissAll();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});

describe('toast queue', () => {
  it('pushes a toast with the right kind and default duration', () => {
    toast.success('Token rotado');
    const [t] = current();
    expect(t.kind).toBe('success');
    expect(t.message).toBe('Token rotado');
    expect(t.duration).toBe(4000);
  });

  it('uses per-kind default durations', () => {
    toast.info('i');
    toast.warning('w');
    toast.error('e');
    const byKind = Object.fromEntries(current().map((t) => [t.kind, t.duration]));
    expect(byKind.info).toBe(4000);
    expect(byKind.warning).toBe(5000);
    expect(byKind.error).toBe(6000);
  });

  it('shows the most recent toast first', () => {
    toast.info('first');
    toast.info('second');
    expect(current().map((t) => t.message)).toEqual(['second', 'first']);
  });

  it('caps the queue at 8 simultaneous toasts', () => {
    for (let i = 0; i < 12; i++) toast.info(`m${i}`);
    expect(current()).toHaveLength(8);
    // Newest kept, oldest dropped.
    expect(current()[0].message).toBe('m11');
    expect(current().some((t) => t.message === 'm0')).toBe(false);
  });

  it('assigns unique, increasing ids', () => {
    const a = toast.info('a');
    const b = toast.info('b');
    expect(b).toBeGreaterThan(a);
  });
});

describe('auto-dismiss', () => {
  it('removes a toast after its duration elapses', () => {
    toast.success('bye'); // 4000ms
    expect(current()).toHaveLength(1);
    vi.advanceTimersByTime(3999);
    expect(current()).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(current()).toHaveLength(0);
  });

  it('keeps a sticky toast (duration 0) forever', () => {
    toast.show({ kind: 'info', message: 'sticky', duration: 0 });
    vi.advanceTimersByTime(60_000);
    expect(current()).toHaveLength(1);
  });
});

describe('dismiss / dismissAll', () => {
  it('dismiss removes only the targeted toast', () => {
    const a = toast.info('a');
    toast.info('b');
    dismiss(a);
    expect(current().map((t) => t.message)).toEqual(['b']);
  });

  it('dismissAll clears the queue and cancels pending timers', () => {
    toast.info('a');
    toast.info('b');
    dismissAll();
    expect(current()).toHaveLength(0);
    // No timer should fire a stale dismiss after clearing.
    vi.advanceTimersByTime(10_000);
    expect(current()).toHaveLength(0);
  });
});

describe('pause / resume dismiss', () => {
  it('pause halts the auto-dismiss countdown', () => {
    const id = toast.success('hover me'); // 4000ms
    pauseDismiss(id);
    vi.advanceTimersByTime(10_000);
    expect(current()).toHaveLength(1);
    // Resuming reschedules the original duration.
    resumeDismiss(id);
    vi.advanceTimersByTime(4000);
    expect(current()).toHaveLength(0);
  });
});

describe('toast.fromError', () => {
  it('uses the Error message as the description', () => {
    toast.fromError('No se pudo guardar', new Error('boom'));
    const [t] = current();
    expect(t.kind).toBe('error');
    expect(t.message).toBe('No se pudo guardar');
    expect(t.description).toBe('boom');
  });

  it('stringifies a non-Error value', () => {
    toast.fromError('fallo', 'raw string');
    expect(current()[0].description).toBe('raw string');
  });
});

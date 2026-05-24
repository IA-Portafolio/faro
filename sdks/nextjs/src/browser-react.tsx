/**
 * React ErrorBoundary que reporta automáticamente a Faro.
 * Importar desde `@iaportafolio/nextjs/client`.
 */
import * as React from 'react';
import { captureException } from './browser-core';

export interface FaroErrorBoundaryProps {
  children: React.ReactNode;
  /** Fallback UI cuando un hijo lanza. Recibe el error y un `reset` para reintentar. */
  fallback?: React.ReactNode | ((args: { error: Error; reset: () => void }) => React.ReactNode);
  /** Tags adicionales para el evento (ej. nombre del módulo) */
  tags?: Record<string, string>;
  /** Hook opcional cuando se captura un error (para tracking adicional) */
  onError?: (error: Error, info: React.ErrorInfo) => void;
}

interface State {
  error: Error | null;
}

export class FaroErrorBoundary extends React.Component<FaroErrorBoundaryProps, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo): void {
    captureException(error, {
      tags: {
        origin: 'react.error-boundary',
        ...(this.props.tags ?? {}),
      },
      message: error.message,
    });
    this.props.onError?.(error, info);
  }

  reset = (): void => {
    this.setState({ error: null });
  };

  render(): React.ReactNode {
    if (this.state.error) {
      const fb = this.props.fallback;
      if (typeof fb === 'function') return fb({ error: this.state.error, reset: this.reset });
      if (fb !== undefined) return fb;
      return null;
    }
    return this.props.children;
  }
}

import * as React from 'react';

/**
 * React bindings — ErrorBoundary que reporta automáticamente a Faro.
 * Import path: `@iaportafolio/browser/react`
 */

interface FaroErrorBoundaryProps {
    children: React.ReactNode;
    /** Fallback UI cuando un hijo lanza. Recibe el error y un `reset` para reintentar. */
    fallback?: React.ReactNode | ((args: {
        error: Error;
        reset: () => void;
    }) => React.ReactNode);
    /** Tags adicionales para el evento (ej. nombre del módulo) */
    tags?: Record<string, string>;
    /** Hook opcional cuando se captura un error (para tracking adicional) */
    onError?: (error: Error, info: React.ErrorInfo) => void;
}
interface State {
    error: Error | null;
}
declare class FaroErrorBoundary extends React.Component<FaroErrorBoundaryProps, State> {
    state: State;
    static getDerivedStateFromError(error: Error): State;
    componentDidCatch(error: Error, info: React.ErrorInfo): void;
    reset: () => void;
    render(): React.ReactNode;
}

export { FaroErrorBoundary, type FaroErrorBoundaryProps };

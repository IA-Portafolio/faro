import {
  captureException
} from "./chunk-3YDKEFQX.js";

// src/react.tsx
import * as React from "react";
var FaroErrorBoundary = class extends React.Component {
  constructor() {
    super(...arguments);
    this.state = { error: null };
    this.reset = () => {
      this.setState({ error: null });
    };
  }
  static getDerivedStateFromError(error) {
    return { error };
  }
  componentDidCatch(error, info) {
    captureException(error, {
      tags: {
        origin: "react.error-boundary",
        ...this.props.tags ?? {}
      },
      message: error.message
    });
    this.props.onError?.(error, info);
  }
  render() {
    if (this.state.error) {
      const fb = this.props.fallback;
      if (typeof fb === "function") return fb({ error: this.state.error, reset: this.reset });
      if (fb !== void 0) return fb;
      return null;
    }
    return this.props.children;
  }
};
export {
  FaroErrorBoundary
};

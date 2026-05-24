"use strict";
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// src/react.tsx
var react_exports = {};
__export(react_exports, {
  FaroErrorBoundary: () => FaroErrorBoundary
});
module.exports = __toCommonJS(react_exports);
var React = __toESM(require("react"), 1);

// src/index.ts
var singleton = null;
function getClient() {
  if (!singleton) throw new Error("faro: init() must be called before use");
  return singleton;
}
function captureException(err, ctx) {
  getClient().captureException(err, ctx);
}

// src/react.tsx
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
// Annotate the CommonJS export names for ESM import in node:
0 && (module.exports = {
  FaroErrorBoundary
});

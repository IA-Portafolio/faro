// src/index.ts
var FaroBrowser = class {
  constructor(opts) {
    this.queue = [];
    this.breadcrumbs = [];
    this.user = null;
    this.timer = null;
    this.cleanup = [];
    this.closed = false;
    this.opts = {
      endpoint: opts.endpoint.replace(/\/$/, ""),
      token: opts.token,
      service: opts.service,
      environment: opts.environment,
      release: opts.release,
      attributes: opts.attributes,
      flushIntervalMs: opts.flushIntervalMs ?? 2e3,
      maxBatchSize: opts.maxBatchSize ?? 100,
      maxQueueSize: opts.maxQueueSize ?? 2e3,
      maxBreadcrumbs: opts.maxBreadcrumbs ?? 30,
      captureUnhandled: opts.captureUnhandled ?? true,
      captureConsole: opts.captureConsole ?? false,
      captureWebVitals: opts.captureWebVitals ?? true,
      captureClicks: opts.captureClicks ?? true,
      captureNavigation: opts.captureNavigation ?? true,
      beforeSend: opts.beforeSend
    };
    if (typeof window === "undefined") {
      return;
    }
    this.timer = setInterval(() => void this.flush(), this.opts.flushIntervalMs);
    if (this.opts.captureUnhandled) this.installErrorHandlers();
    if (this.opts.captureConsole) this.installConsoleCapture();
    if (this.opts.captureWebVitals) this.installWebVitals();
    if (this.opts.captureClicks) this.installClickTracking();
    if (this.opts.captureNavigation) this.installNavigationTracking();
    this.installLifecycleHooks();
  }
  // ---------- API pública ----------
  setUser(user) {
    this.user = user;
  }
  addBreadcrumb(crumb) {
    if (this.breadcrumbs.length >= this.opts.maxBreadcrumbs) {
      this.breadcrumbs.shift();
    }
    this.breadcrumbs.push({ ...crumb, timestamp: Date.now() });
  }
  log(entry) {
    if (this.closed) return;
    const attrs = this.composeAttributes(entry.attributes);
    const evt = {
      level: entry.level ?? "INFO",
      message: entry.message,
      timestamp: (/* @__PURE__ */ new Date()).toISOString(),
      attributes: attrs,
      trace_id: entry.trace_id,
      span_id: entry.span_id
    };
    this.enqueue(evt);
  }
  info(message, attrs) {
    this.log({ level: "INFO", message, attributes: attrs });
  }
  warn(message, attrs) {
    this.log({ level: "WARN", message, attributes: attrs });
  }
  error(message, attrs) {
    this.log({ level: "ERROR", message, attributes: attrs });
  }
  captureException(err, ctx) {
    const e = toError(err);
    this.log({
      level: "ERROR",
      message: ctx?.message ?? `${e.name}: ${e.message}`,
      attributes: {
        "exception.type": e.name,
        "exception.message": e.message,
        "exception.stacktrace": e.stack ?? "",
        ...ctx?.tags ?? {}
      }
    });
  }
  async flush(useBeacon = false) {
    if (this.queue.length === 0) return;
    const batch = this.queue.splice(0, this.opts.maxBatchSize);
    const body = JSON.stringify({ service: this.opts.service, logs: batch });
    const url = `${this.opts.endpoint}/api/v1/ingest/logs`;
    if (useBeacon && typeof navigator !== "undefined" && typeof navigator.sendBeacon === "function") {
      const beaconUrl = `${url}?_token=${encodeURIComponent(this.opts.token)}`;
      const ok = navigator.sendBeacon(beaconUrl, new Blob([body], { type: "application/json" }));
      if (ok) return;
    }
    try {
      const res = await fetch(url, {
        method: "POST",
        keepalive: true,
        headers: {
          "Authorization": `Bearer ${this.opts.token}`,
          "Content-Type": "application/json"
        },
        body
      });
      if (!res.ok && res.status >= 500) {
        this.queue.unshift(...batch);
      }
    } catch {
      this.queue.unshift(...batch);
    }
  }
  close() {
    if (this.closed) return;
    this.closed = true;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    for (const fn of this.cleanup) fn();
    this.cleanup = [];
    void this.flush(true);
  }
  // ---------- Internals ----------
  enqueue(evt) {
    const processed = this.opts.beforeSend ? this.opts.beforeSend(evt) : evt;
    if (!processed) return;
    if (this.queue.length >= this.opts.maxQueueSize) return;
    this.queue.push(processed);
  }
  composeAttributes(extra) {
    const attrs = {};
    if (this.opts.attributes) {
      for (const [k, v] of Object.entries(this.opts.attributes)) attrs[k] = String(v);
    }
    if (this.opts.environment) attrs["deployment.environment"] = this.opts.environment;
    if (this.opts.release) attrs["service.version"] = this.opts.release;
    if (typeof window !== "undefined") {
      attrs["browser.url"] = window.location.href;
      attrs["browser.userAgent"] = navigator.userAgent;
    }
    if (this.user) {
      if (this.user.id) attrs["user.id"] = this.user.id;
      if (this.user.email) attrs["user.email"] = this.user.email;
      if (this.user.username) attrs["user.name"] = this.user.username;
    }
    if (this.breadcrumbs.length > 0) {
      attrs["breadcrumbs"] = JSON.stringify(this.breadcrumbs.slice(-this.opts.maxBreadcrumbs));
    }
    if (extra) {
      for (const [k, v] of Object.entries(extra)) {
        attrs[k] = typeof v === "string" ? v : JSON.stringify(v);
      }
    }
    return attrs;
  }
  installErrorHandlers() {
    const onError = (ev) => {
      this.captureException(ev.error ?? ev.message, {
        tags: { origin: "window.error", "source.file": ev.filename ?? "", "source.line": String(ev.lineno ?? 0) }
      });
    };
    const onRejection = (ev) => {
      this.captureException(ev.reason, { tags: { origin: "unhandledrejection" } });
    };
    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onRejection);
    this.cleanup.push(() => window.removeEventListener("error", onError));
    this.cleanup.push(() => window.removeEventListener("unhandledrejection", onRejection));
  }
  installConsoleCapture() {
    const orig = { error: console.error, warn: console.warn };
    console.error = (...args) => {
      this.addBreadcrumb({ category: "console", message: String(args[0] ?? ""), data: { level: "error" } });
      this.log({ level: "ERROR", message: stringifyArgs(args), attributes: { "console.method": "error" } });
      orig.error.apply(console, args);
    };
    console.warn = (...args) => {
      this.addBreadcrumb({ category: "console", message: String(args[0] ?? ""), data: { level: "warn" } });
      orig.warn.apply(console, args);
    };
    this.cleanup.push(() => {
      console.error = orig.error;
      console.warn = orig.warn;
    });
  }
  installWebVitals() {
    void import("web-vitals").then(({ onLCP, onCLS, onINP, onFCP, onTTFB }) => {
      const report = (name) => (m) => {
        this.log({
          level: "INFO",
          message: `web-vital ${name}`,
          attributes: {
            "metric.name": name,
            "metric.value": m.value,
            "metric.rating": m.rating,
            "metric.id": m.id
          }
        });
      };
      onLCP(report("LCP"));
      onCLS(report("CLS"));
      onINP(report("INP"));
      onFCP(report("FCP"));
      onTTFB(report("TTFB"));
    }).catch(() => {
    });
  }
  installClickTracking() {
    const onClick = (ev) => {
      const target = ev.target;
      if (!target) return;
      const tag = target.tagName?.toLowerCase() ?? "";
      const id = target.id;
      const text = (target.textContent ?? "").trim().slice(0, 60);
      const data = { tag };
      if (id) data.id = id;
      if (text) data.text = text;
      this.addBreadcrumb({ category: "click", message: `${tag}${id ? "#" + id : ""}`, data });
    };
    window.addEventListener("click", onClick, { capture: true, passive: true });
    this.cleanup.push(() => window.removeEventListener("click", onClick, { capture: true }));
  }
  installNavigationTracking() {
    const log2 = (from, to, method) => {
      if (from === to) return;
      this.addBreadcrumb({ category: "navigation", message: `${from} \u2192 ${to}`, data: { method, to } });
    };
    const origPush = history.pushState;
    const origReplace = history.replaceState;
    history.pushState = function(...args) {
      const from = location.href;
      const ret = origPush.apply(this, args);
      log2(from, location.href, "pushState");
      return ret;
    };
    history.replaceState = function(...args) {
      const from = location.href;
      const ret = origReplace.apply(this, args);
      log2(from, location.href, "replaceState");
      return ret;
    };
    const onPop = () => log2("", location.href, "popstate");
    window.addEventListener("popstate", onPop);
    this.cleanup.push(() => {
      history.pushState = origPush;
      history.replaceState = origReplace;
      window.removeEventListener("popstate", onPop);
    });
  }
  installLifecycleHooks() {
    const onHide = () => {
      if (document.visibilityState === "hidden") void this.flush(true);
    };
    const onPageHide = () => void this.flush(true);
    document.addEventListener("visibilitychange", onHide);
    window.addEventListener("pagehide", onPageHide);
    this.cleanup.push(() => document.removeEventListener("visibilitychange", onHide));
    this.cleanup.push(() => window.removeEventListener("pagehide", onPageHide));
  }
};
function toError(err) {
  if (err instanceof Error) return err;
  if (typeof err === "string") return new Error(err);
  try {
    return new Error(JSON.stringify(err));
  } catch {
    return new Error(String(err));
  }
}
function stringifyArgs(args) {
  return args.map((a) => typeof a === "string" ? a : a instanceof Error ? a.stack ?? a.message : safeJson(a)).join(" ");
}
function safeJson(v) {
  try {
    return JSON.stringify(v);
  } catch {
    return String(v);
  }
}
var singleton = null;
function init(opts) {
  if (singleton) singleton.close();
  singleton = new FaroBrowser(opts);
  return singleton;
}
function getClient() {
  if (!singleton) throw new Error("faro: init() must be called before use");
  return singleton;
}
function log(entry) {
  getClient().log(entry);
}
function info(msg, attrs) {
  getClient().info(msg, attrs);
}
function warn(msg, attrs) {
  getClient().warn(msg, attrs);
}
function error(msg, attrs) {
  getClient().error(msg, attrs);
}
function captureException(err, ctx) {
  getClient().captureException(err, ctx);
}
function setUser(user) {
  getClient().setUser(user);
}
function addBreadcrumb(crumb) {
  getClient().addBreadcrumb(crumb);
}
function flush() {
  return getClient().flush();
}
function close() {
  getClient().close();
}

export {
  FaroBrowser,
  init,
  getClient,
  log,
  info,
  warn,
  error,
  captureException,
  setUser,
  addBreadcrumb,
  flush,
  close
};

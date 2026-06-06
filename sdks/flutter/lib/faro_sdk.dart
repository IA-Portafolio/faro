/// SDK de Faro para Flutter / Dart.
///
/// Uso:
/// ```dart
/// import 'package:faro_sdk/faro_sdk.dart';
///
/// void main() {
///   Faro.run(
///     options: const FaroOptions(
///       endpoint: 'https://faro.iaportafolio.com',
///       token: '...',
///       service: 'mi-app-flutter',
///       environment: 'production',
///     ),
///     appRunner: () => runApp(const MyApp()),
///   );
/// }
/// ```
library faro_sdk;

import 'dart:async';
import 'dart:convert';
import 'dart:developer' as developer;

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:http/http.dart' as http;

/// Payload exacto que sale por la red (post-merge de atributos + post-scrub).
/// Es lo que recibe [FaroOptions.beforeSend] y lo que se puede mutar/descartar.
class WireEntry {
  String level;
  String message;
  String timestamp;
  Map<String, String> attributes;
  String? traceId;
  String? spanId;
  WireEntry({
    required this.level,
    required this.message,
    required this.timestamp,
    required this.attributes,
    this.traceId,
    this.spanId,
  });

  Map<String, dynamic> toJson() => {
        'level': level,
        'message': message,
        'timestamp': timestamp,
        if (traceId != null) 'trace_id': traceId,
        if (spanId != null) 'span_id': spanId,
        'attributes': attributes,
      };
}

/// Presets de regex disponibles en [FaroOptions.scrubPatterns].
const _kDefaultScrubFields = <String>[
  'password', 'token', 'secret', 'authorization', 'cookie', 'set-cookie', 'api_key', 'apikey',
];
const _kHeaderScrubFields = <String>['authorization', 'cookie', 'set-cookie'];
const _kRedacted = '[REDACTED]';

final Map<String, RegExp> _scrubRegexes = {
  'email': RegExp(r'[\w.+-]+@[\w-]+(?:\.[\w-]+)+'),
  'jwt': RegExp(r'\beyJ[\w-]+\.[\w-]+\.[\w-]+\b'),
  // Sin Luhn; opt-in deliberadamente.
  'credit-card': RegExp(r'\b(?:\d[ -]?){13,19}\b'),
  'api-key': RegExp(
    r'\b(?:sk-|ghp_|ghs_|gho_|github_pat_|xoxb-|xoxp-|xoxs-|AKIA|ASIA|AIza)[\w-]{12,}\b',
  ),
};

class FaroOptions {
  final String endpoint;
  final String token;
  final String service;
  final String? environment;
  final String? release;
  final Map<String, String> attributes;
  final Duration flushInterval;
  final int maxBatchSize;
  final int maxQueueSize;
  final bool installGlobalHandlers;
  final Duration httpTimeout;
  final List<String> scrubFields;
  final bool scrubHeaders;
  final List<String> scrubPatterns;
  final WireEntry? Function(WireEntry entry)? beforeSend;
  /// Cadencia de refresh de feature flags (por defecto 30s). Port del
  /// `featureFlagRefreshIntervalMs` de Node.
  final Duration featureFlagRefreshInterval;

  const FaroOptions({
    required this.endpoint,
    required this.token,
    required this.service,
    this.environment,
    this.release,
    this.attributes = const {},
    // Perfil de defaults: "mobile" (sdks/README.md → Perfiles de defaults).
    this.flushInterval = const Duration(milliseconds: 1500),
    this.maxBatchSize = 100,
    this.maxQueueSize = 5000,
    this.installGlobalHandlers = true,
    this.httpTimeout = const Duration(seconds: 8),
    this.scrubFields = _kDefaultScrubFields,
    this.scrubHeaders = true,
    this.scrubPatterns = const ['jwt', 'api-key'],
    this.beforeSend,
    this.featureFlagRefreshInterval = const Duration(seconds: 30),
  });
}

// El antiguo `_Entry` se reemplaza por `WireEntry` (público, para beforeSend).

/// Payload de un product event (track / identify / page / screen / alias).
/// Se envía a `POST /api/v1/ingest/events` y persiste en `faro.product_events`.
class ProductEventWire {
  final String type;
  final String name;
  final String timestamp;
  final String distinctId;
  final String anonymousId;
  final String sessionId;
  final Map<String, dynamic> properties;
  final Map<String, dynamic> userProperties;
  final Map<String, dynamic> context;
  final String source;

  const ProductEventWire({
    required this.type,
    required this.name,
    required this.timestamp,
    required this.distinctId,
    required this.anonymousId,
    required this.sessionId,
    required this.properties,
    required this.userProperties,
    required this.context,
    required this.source,
  });

  Map<String, dynamic> toJson() => {
        'type': type,
        'name': name,
        'timestamp': timestamp,
        'distinct_id': distinctId,
        'anonymous_id': anonymousId,
        'session_id': sessionId,
        'properties': properties,
        'user_properties': userProperties,
        'context': context,
        'source': source,
      };
}

/// Definición interna de un feature flag tal como llega del backend
/// (`GET /api/v1/ingest/feature-flags`). Port 1:1 del `FeatureFlagWire` de Node.
class _FlagDef {
  final String key;
  final int rolloutPercentage;
  final Map<String, Object?> conditions;
  const _FlagDef({
    required this.key,
    required this.rolloutPercentage,
    required this.conditions,
  });
}

class Faro {
  static Faro? _instance;
  static Faro get instance =>
      _instance ?? (throw StateError('Hay que llamar primero a Faro.init()'));

  final FaroOptions options;
  final List<WireEntry> _queue = [];
  final List<ProductEventWire> _eventsQueue = [];
  Timer? _timer;
  Timer? _featureFlagsTimer;
  Map<String, _FlagDef> _featureFlags = {};
  String _featureFlagsProject = '';
  final Set<String> _featureExposureSeen = {};
  bool _closed = false;
  late final List<String> _scrubNeedles;
  late final List<RegExp> _scrubRegexesActive;
  _LifecycleObserver? _lifecycleObserver;
  String _distinctId = '';
  /// anonymous_id estable durante el lifetime del proceso. Cuando aterricemos
  /// una fase 2 con `shared_preferences`, lo persistiremos entre reinicios para
  /// que `alias()` pueda fusionar sesiones offline. Por ahora se regenera.
  late final String _anonymousId =
      'anon_${DateTime.now().microsecondsSinceEpoch.toRadixString(36)}_${_randSuffix()}';
  final Map<String, dynamic> _userProperties = {};

  Faro._(this.options) {
    final needles = <String>{...options.scrubFields.map((s) => s.toLowerCase())};
    if (options.scrubHeaders) needles.addAll(_kHeaderScrubFields);
    _scrubNeedles = needles.toList();
    _scrubRegexesActive = options.scrubPatterns
        .map((p) => _scrubRegexes[p])
        .whereType<RegExp>()
        .toList(growable: false);
  }

  /// Inicializa el SDK manualmente. En apps Flutter prefiere [Faro.run] porque
  /// también instala el guard de errores asíncronos basado en zonas.
  static Faro init(FaroOptions options) {
    // Paridad cross-SDK: Python/Go/Node lanzan un error claro en el mismo caso.
    if (options.endpoint.isEmpty) {
      throw ArgumentError("faro.init: 'endpoint' es obligatorio (string no vacío)");
    }
    if (options.token.isEmpty) {
      throw ArgumentError("faro.init: 'token' es obligatorio (string no vacío)");
    }
    if (options.service.isEmpty) {
      throw ArgumentError("faro.init: 'service' es obligatorio (string no vacío)");
    }
    if (_instance != null) _instance!.close();
    final f = Faro._(options);
    f._start();
    _instance = f;
    return f;
  }

  /// Punto de entrada recomendado para apps Flutter. Envuelve [appRunner] en una
  /// zona protegida para capturar también los errores asíncronos fuera del framework.
  static Future<void> run({
    required FaroOptions options,
    required FutureOr<void> Function() appRunner,
  }) async {
    final faro = init(options);
    if (options.installGlobalHandlers) faro._installFlutterHandlers();
    runZonedGuarded(() async {
      await appRunner();
    }, (err, stack) {
      faro.captureException(err, stack: stack, tags: {'origin': 'zone'});
    });
  }

  void log({
    String level = 'INFO',
    required String message,
    Map<String, dynamic>? attributes,
    String? traceId,
    String? spanId,
  }) {
    if (_closed) return;
    final attrs = <String, String>{};
    attrs.addAll(options.attributes);
    if (options.environment != null) attrs['deployment.environment'] = options.environment!;
    if (options.release != null) attrs['service.version'] = options.release!;
    if (attributes != null) {
      attributes.forEach((k, v) {
        attrs[k] = v is String ? v : jsonEncode(v);
      });
    }
    var wire = WireEntry(
      level: level.toUpperCase(),
      message: message,
      timestamp: DateTime.now().toUtc().toIso8601String(),
      attributes: attrs,
      traceId: traceId,
      spanId: spanId,
    );
    _scrub(wire);
    if (options.beforeSend != null) {
      final out = options.beforeSend!(wire);
      if (out == null) return;
      wire = out;
    }
    if (_queue.length >= options.maxQueueSize) {
      developer.log('cola llena, descartando', name: 'faro');
      return;
    }
    _queue.add(wire);
  }

  void _scrub(WireEntry e) {
    e.attributes.forEach((k, v) {
      final kLower = k.toLowerCase();
      if (_scrubNeedles.any((n) => kLower.contains(n))) {
        e.attributes[k] = _kRedacted;
      } else if (_scrubRegexesActive.isNotEmpty) {
        var nv = v;
        for (final rx in _scrubRegexesActive) {
          nv = nv.replaceAll(rx, _kRedacted);
        }
        e.attributes[k] = nv;
      }
    });
    if (_scrubRegexesActive.isNotEmpty) {
      var m = e.message;
      for (final rx in _scrubRegexesActive) {
        m = m.replaceAll(rx, _kRedacted);
      }
      e.message = m;
    }
  }

  // ---------- Product events API (Segment/PostHog-like) ----------

  /// Envía un evento custom de producto.
  void track(String eventName, [Map<String, dynamic>? properties]) {
    _enqueueEvent('track', eventName, properties ?? {});
  }

  /// Identifica al usuario actual: setea `distinct_id` y emite `$identify`.
  void identify(String userId, [Map<String, dynamic>? traits]) {
    if (userId.isEmpty) return;
    _distinctId = userId;
    if (traits != null) _userProperties.addAll(traits);
    _enqueueEvent(
      'identify',
      r'$identify',
      const {},
      userPropertiesOverride: traits ?? {},
    );
  }

  /// Page view (apps Flutter web).
  void page(String path, [Map<String, dynamic>? properties]) {
    _enqueueEvent('page', path, properties ?? {});
  }

  /// Screen view (apps Flutter mobile). Por paridad con la API móvil estándar.
  void screen(String screenName, [Map<String, dynamic>? properties]) {
    _enqueueEvent('screen', screenName, properties ?? {});
  }

  /// Fusiona una sesión pre-login (`prevId`) con un usuario post-login (`newId`).
  void alias(String prevId, String newId) {
    if (prevId.isEmpty || newId.isEmpty) return;
    _distinctId = newId;
    _enqueueEvent(
      'alias',
      r'$alias',
      const {},
      anonymousIdOverride: prevId,
    );
  }

  void _enqueueEvent(
    String type,
    String name,
    Map<String, dynamic> properties, {
    Map<String, dynamic>? userPropertiesOverride,
    String? anonymousIdOverride,
    String? distinctIdOverride,
  }) {
    if (_closed) return;
    if (_eventsQueue.length >= options.maxQueueSize) return;
    final ctx = <String, dynamic>{};
    if (options.environment != null) ctx['environment'] = options.environment;
    if (options.release != null) ctx['release'] = options.release;
    ctx.addAll(options.attributes);
    _eventsQueue.add(ProductEventWire(
      type: type,
      name: name,
      timestamp: DateTime.now().toUtc().toIso8601String(),
      distinctId: distinctIdOverride ?? (_distinctId.isNotEmpty ? _distinctId : _anonymousId),
      anonymousId: anonymousIdOverride ?? _anonymousId,
      sessionId: '',
      properties: properties,
      userProperties: userPropertiesOverride ?? Map<String, dynamic>.from(_userProperties),
      context: ctx,
      source: 'mobile',
    ),);
  }

  /// Helper interno para sufijo aleatorio en el anonymous_id. No criptográfico.
  static String _randSuffix() {
    final n = DateTime.now().microsecondsSinceEpoch ^ identityHashCode(Object());
    return n.abs().toRadixString(36).padLeft(8, '0').substring(0, 8);
  }

  // ---------- Feature flags (port 1:1 del SDK de Node) ----------

  /// Refresca la tabla de feature flags desde
  /// `GET {endpoint}/api/v1/ingest/feature-flags`. Nunca lanza hacia el usuario:
  /// ante cualquier error de red/parsing deja un diag log y conserva los flags
  /// actuales. Lo invoca un `Timer.periodic` arrancado en [_start].
  Future<void> refreshFeatureFlags() async {
    if (_closed) return;
    try {
      final res = await http
          .get(
            Uri.parse('${_normalize(options.endpoint)}/api/v1/ingest/feature-flags'),
            headers: {'Authorization': 'Bearer ${options.token}'},
          )
          .timeout(options.httpTimeout);
      if (res.statusCode >= 400) {
        developer.log(
          'feature flags HTTP ${res.statusCode}: ${res.body}',
          name: 'faro',
        );
        return;
      }
      final dynamic body = jsonDecode(res.body);
      if (body is! Map || body['flags'] is! List) {
        developer.log('feature flags response inválida', name: 'faro');
        return;
      }
      final next = <String, _FlagDef>{};
      for (final dynamic raw in body['flags'] as List) {
        if (raw is! Map) continue;
        final key = raw['key'];
        if (key is! String || key.isEmpty) continue;
        final conditions = raw['conditions'];
        next[key] = _FlagDef(
          key: key,
          rolloutPercentage: _clamp(_asInt(raw['rollout_percentage'])),
          conditions: conditions is Map
              ? conditions.map((k, v) => MapEntry(k.toString(), v))
              : <String, Object?>{},
        );
      }
      _featureFlags = next;
      _featureFlagsProject = body['project'] is String ? body['project'] as String : '';
    } catch (e) {
      developer.log('falló el refresh de feature flags: $e', name: 'faro');
    }
  }

  /// Evalúa un feature flag de forma determinista (sticky bucketing FNV-1a).
  /// Emite a lo sumo un `$feature_exposure` por (flag, distinct_id, variante).
  bool isFeatureEnabled(
    String key, {
    String? distinctId,
    Map<String, Object?>? properties,
  }) {
    final flag = _featureFlags[key];
    if (flag == null) return false;
    if (!_matchesConditions(flag, properties)) return false;
    final rollout = _clamp(flag.rolloutPercentage);
    final id = distinctId ?? (_distinctId.isNotEmpty ? _distinctId : _anonymousId);
    final enabled = rollout >= 100 ||
        (rollout > 0 && _stickyBucket('$_featureFlagsProject:$key:$id') < rollout);
    _trackFeatureExposure(key, id, enabled);
    return enabled;
  }

  bool _matchesConditions(_FlagDef flag, Map<String, Object?>? properties) {
    final required = flag.conditions['properties'];
    if (required is! Map) return true;
    final props = properties ?? const <String, Object?>{};
    for (final entry in required.entries) {
      if (props[entry.key.toString()] != entry.value) return false;
    }
    return true;
  }

  void _trackFeatureExposure(String flagKey, String distinctId, bool enabled) {
    final variant = enabled ? 'B' : 'A';
    final dedup = '$_featureFlagsProject:$flagKey:$distinctId:$variant';
    if (!_featureExposureSeen.add(dedup)) return;
    _enqueueEvent(
      'track',
      r'$feature_exposure',
      {'flag_key': flagKey, 'variant': variant, 'enabled': enabled},
      distinctIdOverride: distinctId,
    );
  }

  int _clamp(int n) => n.clamp(0, 100);

  static int _asInt(Object? v) {
    if (v is int) return v;
    if (v is double) return v.truncate();
    if (v is num) return v.toInt();
    return 0;
  }

  /// FNV-1a 32-bit. Itera sobre `codeUnits` (UTF-16) y enmascara a 32-bit en cada
  /// paso para que el resultado sea idéntico en VM (int 64-bit) y en web (int 53-bit).
  int _stickyBucket(String s) {
    int h = 0x811c9dc5;
    for (final c in s.codeUnits) {
      h = (h ^ c) & 0xFFFFFFFF;
      h = (h * 0x01000193) & 0xFFFFFFFF;
    }
    return h % 100;
  }

  void info(String message, [Map<String, dynamic>? attrs]) =>
      log(level: 'INFO', message: message, attributes: attrs);
  void warn(String message, [Map<String, dynamic>? attrs]) =>
      log(level: 'WARN', message: message, attributes: attrs);
  /// Alias de [warn] — por paridad con `logging.WARNING` / SDK de Python.
  void warning(String message, [Map<String, dynamic>? attrs]) =>
      log(level: 'WARN', message: message, attributes: attrs);
  void error(String message, [Map<String, dynamic>? attrs]) =>
      log(level: 'ERROR', message: message, attributes: attrs);

  void captureException(
    Object exception, {
    StackTrace? stack,
    Map<String, String>? tags,
    String? message,
  }) {
    final attrs = <String, dynamic>{
      'exception.type': exception.runtimeType.toString(),
      'exception.message': exception.toString(),
      'exception.stacktrace': (stack ?? StackTrace.current).toString(),
    };
    if (tags != null) attrs.addAll(tags);
    log(
      level: 'ERROR',
      message: message ?? '${exception.runtimeType}: $exception',
      attributes: attrs,
    );
  }

  Future<void> flush() async {
    await Future.wait([_flushLogs(), _flushEvents()]);
  }

  Future<void> _flushLogs() async {
    if (_queue.isEmpty) return;
    final batch = _queue.take(options.maxBatchSize).toList();
    _queue.removeRange(0, batch.length);
    final body = jsonEncode({
      'service': options.service,
      'logs': batch.map((e) => e.toJson()).toList(),
    });
    try {
      final res = await http
          .post(
            Uri.parse('${_normalize(options.endpoint)}/api/v1/ingest/logs'),
            headers: {
              'Authorization': 'Bearer ${options.token}',
              'Content-Type': 'application/json',
            },
            body: body,
          )
          .timeout(options.httpTimeout);
      if (res.statusCode >= 400) {
        developer.log('ingest ${res.statusCode}: ${res.body}', name: 'faro');
        // Reinserta para que un flush futuro reintente.
        _queue.insertAll(0, batch);
      }
    } catch (e, _) {
      _queue.insertAll(0, batch);
      developer.log('falló el flush: $e', name: 'faro');
    }
  }

  Future<void> _flushEvents() async {
    if (_eventsQueue.isEmpty) return;
    final batch = _eventsQueue.take(options.maxBatchSize).toList();
    _eventsQueue.removeRange(0, batch.length);
    final body = jsonEncode({
      'service': options.service,
      'events': batch.map((e) => e.toJson()).toList(),
    });
    try {
      final res = await http
          .post(
            Uri.parse('${_normalize(options.endpoint)}/api/v1/ingest/events'),
            headers: {
              'Authorization': 'Bearer ${options.token}',
              'Content-Type': 'application/json',
            },
            body: body,
          )
          .timeout(options.httpTimeout);
      if (res.statusCode >= 400) {
        developer.log('ingest events ${res.statusCode}: ${res.body}', name: 'faro');
        _eventsQueue.insertAll(0, batch);
      }
    } catch (e, _) {
      _eventsQueue.insertAll(0, batch);
      developer.log('falló el flush de events: $e', name: 'faro');
    }
  }

  /// Drena las colas y cierra. El [timeout] acota el peor caso (red caída + cola
  /// llena): si vence, se corta el drenado y se pierde a lo sumo lo que quede en
  /// cola (un batch incompleto), como documenta `sdks/README.md`. Cancela los
  /// timers (flush + feature flags) y restaura los handlers de lifecycle.
  Future<void> close({Duration timeout = const Duration(seconds: 5)}) async {
    if (_closed) return;
    _closed = true;
    _timer?.cancel();
    _featureFlagsTimer?.cancel();
    if (_lifecycleObserver != null) {
      WidgetsBinding.instance.removeObserver(_lifecycleObserver!);
      _lifecycleObserver = null;
    }
    final deadline = DateTime.now().add(timeout);
    while (_queue.isNotEmpty || _eventsQueue.isNotEmpty) {
      if (!DateTime.now().isBefore(deadline)) break;
      final before = _queue.length + _eventsQueue.length;
      // Acota cada flush al tiempo restante: si la red cuelga, no bloquea más
      // allá del deadline global.
      final remaining = deadline.difference(DateTime.now());
      if (remaining <= Duration.zero) break;
      var timedOut = false;
      await Future.any<void>([
        flush(),
        Future<void>.delayed(remaining).then((_) => timedOut = true),
      ]);
      if (timedOut) break;
      // Red caída → la cola no se reduce: no insistir (paridad con Node).
      if (_queue.length + _eventsQueue.length >= before) break;
    }
  }

  void _start() {
    _timer = Timer.periodic(options.flushInterval, (_) => flush());
    // Refresh periódico de feature flags. Sin fetch inicial inmediato (paridad Node):
    // el primer refresh ocurre tras `featureFlagRefreshInterval`. Quien necesite
    // los flags al arrancar puede llamar a `refreshFeatureFlags()` manualmente.
    _featureFlagsTimer = Timer.periodic(
      options.featureFlagRefreshInterval,
      (_) => refreshFeatureFlags(),
    );
  }

  void _installFlutterHandlers() {
    // Errores dentro del framework de Flutter (build/layout/paint/gesture).
    final prevFlutter = FlutterError.onError;
    FlutterError.onError = (FlutterErrorDetails details) {
      captureException(
        details.exception,
        stack: details.stack,
        tags: {
          'origin': 'flutter',
          if (details.library != null) 'library': details.library!,
        },
      );
      prevFlutter?.call(details);
    };

    // Errores que escapan del framework (a nivel de isolate de Dart — Flutter 3.3+).
    final prevPlatform = PlatformDispatcher.instance.onError;
    PlatformDispatcher.instance.onError = (error, stack) {
      captureException(error, stack: stack, tags: {'origin': 'platform'});
      return prevPlatform?.call(error, stack) ?? false;
    };

    // Lifecycle: cuando la app pasa a background el SO puede matarla en cualquier momento.
    // Hacer flush en paused/hidden/detached evita perder eventos pendientes.
    _lifecycleObserver = _LifecycleObserver(() => flush());
    WidgetsBinding.instance.addObserver(_lifecycleObserver!);
  }
}

/// Observer privado que invoca un callback cuando la app pierde foreground.
/// Pensado para drenar el buffer del SDK antes de que el SO mate el proceso.
class _LifecycleObserver with WidgetsBindingObserver {
  final void Function() onLeavingForeground;
  _LifecycleObserver(this.onLeavingForeground);

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    // En Android paused = app en background; en iOS inactive precede a paused.
    // hidden y detached (Flutter 3.13+) cubren los estados terminales.
    if (state == AppLifecycleState.paused ||
        state == AppLifecycleState.hidden ||
        state == AppLifecycleState.detached) {
      onLeavingForeground();
    }
  }
}

String _normalize(String s) => s.endsWith('/') ? s.substring(0, s.length - 1) : s;

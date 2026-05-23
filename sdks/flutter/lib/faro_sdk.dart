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
import 'package:http/http.dart' as http;

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

  const FaroOptions({
    required this.endpoint,
    required this.token,
    required this.service,
    this.environment,
    this.release,
    this.attributes = const {},
    this.flushInterval = const Duration(milliseconds: 1500),
    this.maxBatchSize = 100,
    this.maxQueueSize = 5000,
    this.installGlobalHandlers = true,
    this.httpTimeout = const Duration(seconds: 8),
  });
}

class _Entry {
  final String level;
  final String message;
  final DateTime timestamp;
  final Map<String, String> attributes;
  final String? traceId;
  final String? spanId;
  _Entry(this.level, this.message, this.attributes, {this.traceId, this.spanId})
      : timestamp = DateTime.now().toUtc();

  Map<String, dynamic> toJson() => {
        'level': level,
        'message': message,
        'timestamp': timestamp.toIso8601String(),
        if (traceId != null) 'trace_id': traceId,
        if (spanId != null) 'span_id': spanId,
        'attributes': attributes,
      };
}

class Faro {
  static Faro? _instance;
  static Faro get instance =>
      _instance ?? (throw StateError('Hay que llamar primero a Faro.init()'));

  final FaroOptions options;
  final List<_Entry> _queue = [];
  Timer? _timer;
  bool _closed = false;

  Faro._(this.options);

  /// Inicializa el SDK manualmente. En apps Flutter prefiere [Faro.run] porque
  /// también instala el guard de errores asíncronos basado en zonas.
  static Faro init(FaroOptions options) {
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
    if (_queue.length >= options.maxQueueSize) {
      developer.log('cola llena, descartando', name: 'faro');
      return;
    }
    _queue.add(_Entry(level.toUpperCase(), message, attrs, traceId: traceId, spanId: spanId));
  }

  void info(String message, [Map<String, dynamic>? attrs]) =>
      log(level: 'INFO', message: message, attributes: attrs);
  void warn(String message, [Map<String, dynamic>? attrs]) =>
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

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    _timer?.cancel();
    while (_queue.isNotEmpty) {
      final before = _queue.length;
      await flush();
      if (_queue.length >= before) break;
    }
  }

  void _start() {
    _timer = Timer.periodic(options.flushInterval, (_) => flush());
  }

  void _installFlutterHandlers() {
    // Errores dentro del framework de Flutter (build/layout/paint/gesture).
    final prevFlutter = FlutterError.onError;
    FlutterError.onError = (FlutterErrorDetails details) {
      captureException(details.exception, stack: details.stack, tags: {
        'origin': 'flutter',
        if (details.library != null) 'library': details.library!,
      });
      prevFlutter?.call(details);
    };

    // Errores que escapan del framework (a nivel de isolate de Dart — Flutter 3.3+).
    final prevPlatform = PlatformDispatcher.instance.onError;
    PlatformDispatcher.instance.onError = (error, stack) {
      captureException(error, stack: stack, tags: {'origin': 'platform'});
      return prevPlatform?.call(error, stack) ?? false;
    };
  }
}

String _normalize(String s) => s.endsWith('/') ? s.substring(0, s.length - 1) : s;

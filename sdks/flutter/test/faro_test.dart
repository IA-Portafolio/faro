/// Tests unitarios del SDK Flutter — las 6 invariantes mínimas:
///   1. init con opts inválidas → ArgumentError claro
///   2. log + flush + assert payload (shape del wire)
///   3. queue cap descarta cuando se llena
///   4. retry on 5xx (re-encolar para reintentar)
///   5. captureException compone shape OTel (exception.type/.message/.stacktrace)
///   6. close() drena la cola antes de devolver
///
/// Para los tests con red levantamos un HttpServer local (dart:io). El SDK
/// no acepta inyección de http.Client; apuntar a 127.0.0.1:puerto es la
/// forma menos invasiva de capturar batches sin tocar red real.
library;

import 'dart:convert';
import 'dart:io';

import 'package:faro_sdk/faro_sdk.dart';
import 'package:flutter_test/flutter_test.dart';

/// Server local: guarda cada request y devuelve `nextStatus` (default 200).
class _CaptureServer {
  late HttpServer _server;
  final List<_CapturedRequest> received = [];
  int nextStatus = 200;

  Future<void> start() async {
    _server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    _server.listen((HttpRequest req) async {
      final body = await utf8.decoder.bind(req).join();
      Map<String, dynamic>? parsed;
      try { parsed = jsonDecode(body) as Map<String, dynamic>; } catch (_) {}
      received.add(
        _CapturedRequest(
          method: req.method,
          path: req.uri.path,
          auth: req.headers.value('authorization') ?? '',
          contentType: req.headers.value('content-type') ?? '',
          body: parsed ?? {},
        ),
      );
      req.response.statusCode = nextStatus;
      req.response.headers.contentType = ContentType.json;
      req.response.write('{"ok":true}');
      await req.response.close();
    });
  }

  String get url => 'http://127.0.0.1:${_server.port}';
  Future<void> stop() => _server.close(force: true);
}

class _CapturedRequest {
  final String method;
  final String path;
  final String auth;
  final String contentType;
  final Map<String, dynamic> body;
  _CapturedRequest({
    required this.method,
    required this.path,
    required this.auth,
    required this.contentType,
    required this.body,
  });
}

/// Espera hasta que `cond` sea true o expire `timeout`.
Future<void> _waitFor(bool Function() cond, {Duration timeout = const Duration(seconds: 2)}) async {
  final deadline = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(deadline)) {
    if (cond()) return;
    await Future<void>.delayed(const Duration(milliseconds: 20));
  }
}

List<Map<String, dynamic>> _productEvents(_CaptureServer server) {
  return server.received
      .expand((req) => ((req.body['events'] as List<dynamic>?) ?? const <dynamic>[]))
      .cast<Map<String, dynamic>>()
      .toList();
}

void main() {
  late _CaptureServer server;

  setUp(() async {
    server = _CaptureServer();
    await server.start();
  });

  tearDown(() async {
    // close() del singleton si quedó vivo entre tests (cada init lo cierra,
    // pero el último test no llama init de nuevo).
    try { await Faro.instance.close(); } catch (_) {}
    await server.stop();
  });

  // ---------- 1. init con opts inválidas ----------

  group('init inválido', () {
    test('endpoint vacío → ArgumentError claro', () {
      expect(
        () => Faro.init(const FaroOptions(endpoint: '', token: 'tk', service: 's')),
        throwsA(isA<ArgumentError>().having((e) => e.message, 'message', contains('endpoint'))),
      );
    });

    test('token vacío → ArgumentError claro', () {
      expect(
        () => Faro.init(const FaroOptions(endpoint: 'http://x', token: '', service: 's')),
        throwsA(isA<ArgumentError>().having((e) => e.message, 'message', contains('token'))),
      );
    });

    test('service vacío → ArgumentError claro', () {
      expect(
        () => Faro.init(const FaroOptions(endpoint: 'http://x', token: 'tk', service: '')),
        throwsA(isA<ArgumentError>().having((e) => e.message, 'message', contains('service'))),
      );
    });
  });

  // ---------- 2. log + flush + assert payload ----------

  test('payload: shape del JSON enviado al wire', () async {
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'mi-token',
        service: 'payload-test',
        environment: 'prod',
        release: 'v1.2.3',
        attributes: const {'region': 'eu-west-1'},
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100), // sin auto-flush
      ),
    );

    faro.log(
      level: 'WARN',
      message: 'algo raro',
      attributes: {'http.status_code': 500, 'user.id': 'u42'},
    );
    await faro.flush();
    await _waitFor(() => server.received.isNotEmpty);

    expect(server.received, hasLength(1));
    final req = server.received.first;
    expect(req.method, 'POST');
    expect(req.path, '/api/v1/ingest/logs');
    expect(req.auth, 'Bearer mi-token');
    expect(req.contentType, contains('application/json'));

    expect(req.body['service'], 'payload-test');
    final logs = req.body['logs'] as List<dynamic>;
    expect(logs, hasLength(1));
    final entry = logs.first as Map<String, dynamic>;
    expect(entry['level'], 'WARN');
    expect(entry['message'], 'algo raro');
    expect(entry['timestamp'], contains('T'));
    final attrs = entry['attributes'] as Map<String, dynamic>;
    expect(attrs['region'], 'eu-west-1');
    expect(attrs['deployment.environment'], 'prod');
    expect(attrs['service.version'], 'v1.2.3');
    // Los no-strings se serializan a JSON (números pasan como string JSON).
    expect(attrs['http.status_code'], '500');
    expect(attrs['user.id'], 'u42');
  });

  // #3 (queue overflow) y #4 (retry) NO están aquí: ya viven en
  // [client_test.dart] con asserts fuertes (cap ≤5 en el batch enviado;
  // 5xx→200 reintento contado). No los duplicamos.

  // ---------- 4. retry on 5xx ----------

  test('5xx: el batch se re-encola para reintentar', () async {
    server.nextStatus = 503;
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'retry-test',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
      ),
    );
    faro.log(level: 'INFO', message: 'reintentar-me');
    await faro.flush(); // → 503, re-encolar
    await _waitFor(() => server.received.isNotEmpty);
    final firstCalls = server.received.length;
    expect(
      firstCalls,
      greaterThanOrEqualTo(1),
      reason: 'debe haber al menos un POST inicial',
    );

    server.nextStatus = 200;
    await faro.flush(); // ahora sí lo acepta
    await _waitFor(() => server.received.length > firstCalls);
    expect(
      server.received.length,
      greaterThan(firstCalls),
      reason: 'tras 5xx → 200 debe haber un reintento',
    );
  });

  // ---------- 4b. Product events API ----------

  test('track: envía evento mobile a /api/v1/ingest/events', () async {
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'track-test',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
      ),
    );

    faro.track('checkout_completed', {'amount': 99.5, 'currency': 'USD'});
    await faro.flush();
    await _waitFor(() => _productEvents(server).isNotEmpty);

    final event = _productEvents(server).first;
    expect(event['type'], 'track');
    expect(event['name'], 'checkout_completed');
    expect(event['properties'], {'amount': 99.5, 'currency': 'USD'});
    expect(event['distinct_id'], startsWith('anon_'));
    expect(event['distinct_id'], event['anonymous_id']);
    expect(event['session_id'], '');
    expect(event['source'], 'mobile');
  });

  test('identify: fija distinct_id para eventos siguientes', () async {
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'identify-test',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
      ),
    );

    faro.identify('user_42', {'email': 'a@b.com', 'plan': 'pro'});
    faro.track('after_login');
    await faro.flush();
    await _waitFor(() => _productEvents(server).length >= 2);

    final events = _productEvents(server);
    final identify = events.firstWhere((e) => e['type'] == 'identify');
    final track = events.firstWhere((e) => e['type'] == 'track');
    expect(identify['name'], r'$identify');
    expect(identify['distinct_id'], 'user_42');
    expect(identify['user_properties'], {'email': 'a@b.com', 'plan': 'pro'});
    expect(track['distinct_id'], 'user_42');
  });

  test('page y screen: emiten vistas con propiedades', () async {
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'view-test',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
      ),
    );

    faro.page('/checkout/success', {'source': 'cart'});
    faro.screen('CheckoutSuccess', {'source': 'cart'});
    await faro.flush();
    await _waitFor(() => _productEvents(server).length >= 2);

    final events = _productEvents(server);
    final page = events.firstWhere((e) => e['type'] == 'page');
    final screen = events.firstWhere((e) => e['type'] == 'screen');
    expect(page['name'], '/checkout/success');
    expect(page['properties'], {'source': 'cart'});
    expect(page['source'], 'mobile');
    expect(screen['name'], 'CheckoutSuccess');
    expect(screen['properties'], {'source': 'cart'});
    expect(screen['source'], 'mobile');
  });

  test('alias: fusiona anonymous_id previo con user post-login', () async {
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'alias-test',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
      ),
    );

    faro.alias('anonymous_abc123', 'user_42');
    faro.track('post_alias');
    await faro.flush();
    await _waitFor(() => _productEvents(server).length >= 2);

    final events = _productEvents(server);
    final alias = events.firstWhere((e) => e['type'] == 'alias');
    final track = events.firstWhere((e) => e['type'] == 'track');
    expect(alias['name'], r'$alias');
    expect(alias['anonymous_id'], 'anonymous_abc123');
    expect(alias['distinct_id'], 'user_42');
    expect(track['distinct_id'], 'user_42');
  });

  // ---------- 5. auto-captura: captureException compone shape OTel ----------
  //
  // El handler instalado por _installFlutterHandlers() requiere WidgetsBinding
  // listo (lo da TestWidgetsFlutterBinding.ensureInitialized()) y un PlatformDispatcher
  // utilizable; en un test puro es frágil. Cubrimos en su lugar el método público
  // que ese handler invoca: captureException(). Si esto funciona, la auto-captura
  // del runtime también — el handler es un wrapper de 3 líneas.

  test('captureException compone exception.{type,message,stacktrace}', () async {
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'auto-capture',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
      ),
    );

    try {
      throw StateError('boom sintético');
    } catch (e, st) {
      faro.captureException(e, stack: st, tags: {'origin': 'test'});
    }
    await faro.flush();
    await _waitFor(() => server.received.isNotEmpty);

    final entry = (server.received.first.body['logs'] as List).first as Map<String, dynamic>;
    expect(entry['level'], 'ERROR');
    expect(entry['message'], contains('boom sintético'));
    final attrs = entry['attributes'] as Map<String, dynamic>;
    expect(attrs['exception.type'], 'StateError');
    expect(attrs['exception.message'], contains('boom sintético'));
    expect(attrs['exception.stacktrace'], isA<String>());
    expect(attrs['exception.stacktrace'], isNot(isEmpty));
    expect(attrs['origin'], 'test');
  });

  // ---------- 6. close() graceful ----------

  test('close: drena la cola antes de devolver — sin pérdida', () async {
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'close-test',
        installGlobalHandlers: false,
        // Intervalo lejano: si no fuera por close(), estos eventos no llegarían.
        flushInterval: const Duration(seconds: 100),
      ),
    );
    for (var i = 0; i < 7; i++) {
      faro.log(level: 'INFO', message: 'evento-$i');
    }
    await faro.close();

    final msgs = <String>[];
    for (final req in server.received) {
      for (final log in req.body['logs'] as List) {
        msgs.add((log as Map<String, dynamic>)['message'] as String);
      }
    }
    expect(msgs, hasLength(7), reason: 'close() debe drenar los 7 eventos en cola');
    expect(msgs, equals(List.generate(7, (i) => 'evento-$i')));
  });
}

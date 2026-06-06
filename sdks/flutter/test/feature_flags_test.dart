/// Tests de feature flags + cierre acotado del SDK Flutter.
///
///   (a) Los 5 vectores dorados del sticky bucket (FNV-1a 32-bit) deben coincidir
///       byte a byte con el resto de SDKs (Node es la referencia).
///   (b) /api/v1/ingest/feature-flags con flag rollout=100 → isFeatureEnabled true
///       y se encola un `$feature_exposure` con variant 'B'.
///   (c) Flag con conditions.properties no satisfechas → false y SIN exposición.
///   (d) close(timeout: ...) retorna aunque la red cuelgue (no bloquea).
///
/// El sticky bucket no se expone públicamente, así que (a) se valida de forma
/// equivalente: configuramos un flag con `project`/`key`/`distinct_id` cuyo bucket
/// conocido cae dentro/fuera de un rollout dado, y comprobamos el booleano. Para
/// el resto levantamos un HttpServer local (dart:io) que sirve el JSON de flags.
library;

import 'dart:convert';
import 'dart:io';

import 'package:faro_sdk/faro_sdk.dart';
import 'package:flutter_test/flutter_test.dart';

/// Server local que distingue rutas:
///  - GET  /api/v1/ingest/feature-flags → devuelve `flagsBody` (JSON de flags)
///  - POST /api/v1/ingest/events|logs  → captura y responde 200
/// Permite además colgar (`hang`) para simular red caída.
class _FlagsServer {
  late HttpServer _server;
  final List<Map<String, dynamic>> events = [];
  Map<String, dynamic> flagsBody = {'project': '', 'flags': <dynamic>[]};
  bool hang = false;

  Future<void> start() async {
    _server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    _server.listen((HttpRequest req) async {
      if (hang) {
        // Nunca responde: simula red colgada (el cliente depende de su timeout).
        return;
      }
      final body = await utf8.decoder.bind(req).join();
      if (req.uri.path == '/api/v1/ingest/feature-flags' &&
          req.method == 'GET') {
        req.response.statusCode = 200;
        req.response.headers.contentType = ContentType.json;
        req.response.write(jsonEncode(flagsBody));
        await req.response.close();
        return;
      }
      // logs/events ingest
      Map<String, dynamic>? parsed;
      try {
        parsed = jsonDecode(body) as Map<String, dynamic>;
      } catch (_) {}
      if (parsed != null) {
        for (final e
            in (parsed['events'] as List<dynamic>? ?? const <dynamic>[])) {
          events.add(e as Map<String, dynamic>);
        }
      }
      req.response.statusCode = 200;
      req.response.headers.contentType = ContentType.json;
      req.response.write('{"ok":true}');
      await req.response.close();
    });
  }

  String get url => 'http://127.0.0.1:${_server.port}';
  Future<void> stop() => _server.close(force: true);
}

Future<void> _waitFor(
  bool Function() cond, {
  Duration timeout = const Duration(seconds: 2),
}) async {
  final deadline = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(deadline)) {
    if (cond()) return;
    await Future<void>.delayed(const Duration(milliseconds: 20));
  }
}

void main() {
  late _FlagsServer server;

  setUp(() async {
    server = _FlagsServer();
    await server.start();
  });

  tearDown(() async {
    try {
      await Faro.instance.close();
    } catch (_) {}
    await server.stop();
  });

  // ---------- (a) Vectores dorados del sticky bucket ----------
  //
  // No exponemos _stickyBucket; lo verificamos vía isFeatureEnabled con un
  // flag cuyo rollout está justo por encima del bucket esperado (→ enabled) y
  // justo en el bucket (→ disabled, porque la condición es bucket < rollout).
  //   "proj:new-checkout:user_42" -> 9
  //   "acme:flag-a:anon_x"        -> 54
  //   "myproj:dark-mode:user_1"   -> 75
  //   "p:k:abcdefghij"            -> 49
  //   "demo:exp1:user_42"         -> 34
  group('sticky bucket — vectores dorados', () {
    final vectors = <List<Object>>[
      ['proj', 'new-checkout', 'user_42', 9],
      ['acme', 'flag-a', 'anon_x', 54],
      ['myproj', 'dark-mode', 'user_1', 75],
      ['p', 'k', 'abcdefghij', 49],
      ['demo', 'exp1', 'user_42', 34],
    ];

    for (final v in vectors) {
      final project = v[0] as String;
      final key = v[1] as String;
      final id = v[2] as String;
      final bucket = v[3] as int;
      test('"$project:$key:$id" -> $bucket', () async {
        server.flagsBody = {
          'project': project,
          'flags': [
            {
              'key': key,
              'rollout_percentage': 100,
              'conditions': <String, Object?>{},
            },
          ],
        };
        final faro = Faro.init(
          FaroOptions(
            endpoint: server.url,
            token: 'tk',
            service: 'ff-golden',
            installGlobalHandlers: false,
            flushInterval: const Duration(seconds: 100),
            featureFlagRefreshInterval: const Duration(seconds: 100),
          ),
        );
        await faro.refreshFeatureFlags();

        // rollout = bucket+1 → bucket < rollout → enabled.
        server.flagsBody = {
          'project': project,
          'flags': [
            {
              'key': key,
              'rollout_percentage': bucket + 1,
              'conditions': <String, Object?>{},
            },
          ],
        };
        await faro.refreshFeatureFlags();
        expect(
          faro.isFeatureEnabled(key, distinctId: id),
          isTrue,
          reason: 'bucket esperado=$bucket, rollout=${bucket + 1} → enabled',
        );

        // rollout = bucket → bucket < rollout es falso → disabled.
        server.flagsBody = {
          'project': project,
          'flags': [
            {
              'key': key,
              'rollout_percentage': bucket,
              'conditions': <String, Object?>{},
            },
          ],
        };
        await faro.refreshFeatureFlags();
        expect(
          faro.isFeatureEnabled(key, distinctId: id),
          isFalse,
          reason: 'bucket esperado=$bucket, rollout=$bucket → disabled',
        );
        await faro.close();
      });
    }
  });

  // ---------- (b) rollout=100 → true + $feature_exposure variant B ----------

  test('rollout=100 → isFeatureEnabled true y encola \$feature_exposure (B)',
      () async {
    server.flagsBody = {
      'project': 'demo',
      'flags': [
        {
          'key': 'new-ui',
          'rollout_percentage': 100,
          'conditions': <String, Object?>{},
        },
      ],
    };
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'ff-rollout',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
        featureFlagRefreshInterval: const Duration(seconds: 100),
      ),
    );
    await faro.refreshFeatureFlags();

    expect(faro.isFeatureEnabled('new-ui', distinctId: 'user_1'), isTrue);
    // Llamarlo otra vez NO debe duplicar la exposición (dedup por variante).
    expect(faro.isFeatureEnabled('new-ui', distinctId: 'user_1'), isTrue);

    await faro.flush();
    await _waitFor(() => server.events.isNotEmpty);

    final exposures =
        server.events.where((e) => e['name'] == r'$feature_exposure').toList();
    expect(exposures, hasLength(1), reason: 'dedup: una sola exposición');
    final exp = exposures.first;
    expect(exp['type'], 'track');
    expect(exp['distinct_id'], 'user_1', reason: 'distinct_id override');
    final props = exp['properties'] as Map<String, dynamic>;
    expect(props['flag_key'], 'new-ui');
    expect(props['variant'], 'B');
    expect(props['enabled'], true);
  });

  // ---------- (c) conditions.properties no satisfechas → false sin exposición ----------

  test('conditions.properties no satisfechas → false y sin exposición',
      () async {
    server.flagsBody = {
      'project': 'demo',
      'flags': [
        {
          'key': 'beta',
          'rollout_percentage': 100,
          'conditions': {
            'properties': {'plan': 'pro'},
          },
        },
      ],
    };
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'ff-cond',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
        featureFlagRefreshInterval: const Duration(seconds: 100),
      ),
    );
    await faro.refreshFeatureFlags();

    // plan != pro → no matchea → false, sin exposición.
    expect(
      faro.isFeatureEnabled(
        'beta',
        distinctId: 'user_1',
        properties: {'plan': 'free'},
      ),
      isFalse,
    );
    // Sanity: con el property correcto sí matchea (rollout 100 → true).
    expect(
      faro.isFeatureEnabled(
        'beta',
        distinctId: 'user_2',
        properties: {'plan': 'pro'},
      ),
      isTrue,
    );

    await faro.flush();
    await _waitFor(() => server.events.isNotEmpty);

    final exposures =
        server.events.where((e) => e['name'] == r'$feature_exposure').toList();
    expect(
      exposures,
      hasLength(1),
      reason: 'solo el caso que matchea genera exposición',
    );
    expect((exposures.first['properties'] as Map)['variant'], 'B');
    expect(exposures.first['distinct_id'], 'user_2');
  });

  // ---------- (d) close(timeout:) retorna aunque la red cuelgue ----------

  test('close(timeout:) no bloquea indefinidamente con la red colgada',
      () async {
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'close-timeout',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
        featureFlagRefreshInterval: const Duration(seconds: 100),
        // httpTimeout alto: si close() respetara solo el http timeout, tardaría.
        httpTimeout: const Duration(seconds: 30),
      ),
    );
    server.hang = true; // a partir de aquí el server no responde nunca
    for (var i = 0; i < 5; i++) {
      faro.log(level: 'INFO', message: 'pendiente-$i');
    }

    final sw = Stopwatch()..start();
    await faro.close(timeout: const Duration(milliseconds: 300));
    sw.stop();

    expect(
      sw.elapsed,
      lessThan(const Duration(seconds: 5)),
      reason: 'close() debe cortar en ~timeout, no esperar al http timeout',
    );
  });

  // ---------- isFeatureEnabled sobre flag inexistente → false ----------

  test('flag desconocido → false', () async {
    final faro = Faro.init(
      FaroOptions(
        endpoint: server.url,
        token: 'tk',
        service: 'ff-unknown',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
        featureFlagRefreshInterval: const Duration(seconds: 100),
      ),
    );
    expect(faro.isFeatureEnabled('nope', distinctId: 'x'), isFalse);
  });
}

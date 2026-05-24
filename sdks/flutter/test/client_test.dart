/// Tests unitarios del SDK Flutter — 4 invariantes:
///   1. queue cap descarta cuando se llena
///   2. retry on 5xx
///   3. beforeSend filtra (null → descartar)
///   4. scrubbing aplica scrubFields + scrubPatterns
///
/// Levantamos un HttpServer real en localhost (puerto 0 = libre) en cada test;
/// no usamos mocks porque la SDK pasa por package:http → dart:io igualmente.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:faro_sdk/faro_sdk.dart';

class _Capture {
  final HttpServer server;
  final List<Map<String, dynamic>> batches = [];
  int calls = 0;
  int nextStatus = 200;

  _Capture._(this.server) {
    server.listen((req) async {
      calls++;
      final raw = await utf8.decoder.bind(req).join();
      try {
        batches.add(jsonDecode(raw) as Map<String, dynamic>);
      } catch (_) {
        batches.add({'_raw': raw});
      }
      req.response.statusCode = nextStatus;
      req.response.headers.contentType = ContentType.json;
      req.response.write('{"ok":true}');
      await req.response.close();
    });
  }

  static Future<_Capture> start() async {
    final s = await HttpServer.bind('127.0.0.1', 0);
    return _Capture._(s);
  }

  Uri get endpoint => Uri.parse('http://127.0.0.1:${server.port}');

  Future<void> close() => server.close(force: true);
}

void main() {
  // ---- 1. queue cap ----
  test('queue cap descarta cuando se llena', () async {
    final cap = await _Capture.start();
    try {
      // maxBatchSize == maxQueueSize → un flush vacía toda la cola.
      // Si el cap se respeta, el server recibe a lo sumo 5 eventos en un único batch.
      final faro = Faro.init(FaroOptions(
        endpoint: cap.endpoint.toString(),
        token: 'tk',
        service: 'queue-cap',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100), // sin auto-flush
        maxBatchSize: 50,
        maxQueueSize: 5,
      ));
      try {
        for (var i = 0; i < 50; i++) {
          faro.log(level: 'INFO', message: 'evento $i');
        }
        await faro.flush();
        // El primer batch (el único, dada la config) debe tener ≤5 eventos.
        await _waitFor(() => cap.batches.isNotEmpty);
        final logs = (cap.batches.first['logs'] as List).cast<dynamic>();
        expect(logs.length, lessThanOrEqualTo(5),
            reason: 'el cap se aplica en log(), nunca enviamos más');
      } finally {
        await faro.close();
      }
    } finally {
      await cap.close();
    }
  });

  // ---- 2. retry on 5xx ----
  test('5xx: el batch se re-encola', () async {
    final cap = await _Capture.start();
    try {
      cap.nextStatus = 503;
      final faro = Faro.init(FaroOptions(
        endpoint: cap.endpoint.toString(),
        token: 'tk',
        service: 'retry-test',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
      ));
      try {
        faro.log(level: 'INFO', message: 'reintentar-me');
        await faro.flush(); // 503 → el SDK re-encola
        cap.nextStatus = 200;
        await faro.flush(); // ahora OK
        await _waitFor(() => cap.calls >= 2);
        expect(cap.calls, greaterThanOrEqualTo(2),
            reason: 'segundo intento debe haber llegado');
      } finally {
        await faro.close();
      }
    } finally {
      await cap.close();
    }
  });

  // ---- 3. beforeSend ----
  test('beforeSend null descarta', () async {
    final cap = await _Capture.start();
    try {
      final faro = Faro.init(FaroOptions(
        endpoint: cap.endpoint.toString(),
        token: 'tk',
        service: 'bs-discard',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
        beforeSend: (e) => e.message.contains('descartar') ? null : e,
      ));
      try {
        faro.log(level: 'INFO', message: 'guardar');
        faro.log(level: 'INFO', message: 'descartar');
        faro.log(level: 'INFO', message: 'guardar también');
        await faro.flush();
        await _waitFor(() => cap.batches.isNotEmpty);
        final msgs = (cap.batches.first['logs'] as List)
            .cast<Map<String, dynamic>>()
            .map((l) => l['message'] as String)
            .toList();
        expect(msgs, equals(['guardar', 'guardar también']));
      } finally {
        await faro.close();
      }
    } finally {
      await cap.close();
    }
  });

  // ---- 4. scrubbing ----
  test('scrubbing aplica scrubFields y scrubPatterns', () async {
    final cap = await _Capture.start();
    try {
      final faro = Faro.init(FaroOptions(
        endpoint: cap.endpoint.toString(),
        token: 'tk',
        service: 'scrub',
        installGlobalHandlers: false,
        flushInterval: const Duration(seconds: 100),
      ));
      try {
        faro.log(
          level: 'INFO',
          message: 'auth con eyJabc.def.ghi y key sk-abcdefghijklmnop',
          attributes: {
            'user.password': 'p4ssw0rd',
            'http.request.header.authorization': 'Bearer x',
            'safe.field': 'visible',
            'embedded': 'ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          },
        );
        await faro.flush();
        await _waitFor(() => cap.batches.isNotEmpty);
        final log = (cap.batches.first['logs'] as List).first
            as Map<String, dynamic>;
        final attrs = log['attributes'] as Map<String, dynamic>;
        expect(attrs['user.password'], '[REDACTED]');
        expect(attrs['http.request.header.authorization'], '[REDACTED]');
        expect(attrs['safe.field'], 'visible');
        expect(attrs['embedded'], '[REDACTED]');
        final msg = log['message'] as String;
        expect(msg.contains('eyJabc'), isFalse,
            reason: 'JWT redactado en message');
        expect(msg.contains('sk-abcdef'), isFalse,
            reason: 'sk-* redactado en message');
      } finally {
        await faro.close();
      }
    } finally {
      await cap.close();
    }
  });
}

Future<void> _waitFor(bool Function() cond,
    {Duration timeout = const Duration(seconds: 2)}) async {
  final deadline = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(deadline)) {
    if (cond()) return;
    await Future<void>.delayed(const Duration(milliseconds: 30));
  }
}

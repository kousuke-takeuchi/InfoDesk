import 'dart:convert';

import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:googleapis_auth/googleapis_auth.dart' as auth;
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:infodesk_mobile/storage/drive_store.dart';
import 'package:infodesk_mobile/storage/google_drive_store.dart';

http.Response json(Object body, [int status = 200]) => http.Response(
  jsonEncode(body),
  status,
  headers: {'content-type': 'application/json; charset=utf-8'},
);

Matcher storageError(StorageErrorKind kind) =>
    isA<StorageError>().having((e) => e.kind, 'kind', kind);

void main() {
  test('list follows nextPageToken and sends spaces=appDataFolder', () async {
    final seen = <Uri>[];
    final client = MockClient((req) async {
      seen.add(req.url);
      expect(req.url.path, '/drive/v3/files');
      expect(req.url.queryParameters['spaces'], 'appDataFolder');
      if (req.url.queryParameters['pageToken'] == null) {
        return json({
          'nextPageToken': 'p2',
          'files': [
            {
              'id': 'a',
              'name': 'manifest.json',
              'modifiedTime': '2026-10-14T03:00:00.000Z',
              'size': '12',
            },
          ],
        });
      }
      expect(req.url.queryParameters['pageToken'], 'p2');
      return json({
        'files': [
          {
            'id': 'b',
            'name': 'day-2026-10-14.json.gz',
            'modifiedTime': '2026-10-14T04:00:00.000Z',
            'size': '34',
          },
        ],
      });
    });
    final files = await GoogleDriveStore.withClient(client).list();
    expect(seen, hasLength(2));
    expect(files.map((f) => f.name), [
      'manifest.json',
      'day-2026-10-14.json.gz',
    ]);
    expect(files.first.id, 'a');
    expect(files.first.size, 12);
    expect(files.first.modifiedTime, '2026-10-14T03:00:00.000Z');
  });

  test('download returns the bytes', () async {
    final client = MockClient((req) async {
      expect(req.url.path, '/drive/v3/files/a');
      expect(req.url.queryParameters['alt'], 'media');
      return http.Response.bytes(
        [1, 2, 3],
        200,
        headers: {'content-type': 'application/octet-stream'},
      );
    });
    final store = GoogleDriveStore.withClient(client);
    final bytes = await store.download(
      const RemoteFile(id: 'a', name: 'x', modifiedTime: '', size: 3),
    );
    expect(bytes, [1, 2, 3]);
  });

  test('create sends multipart with parents appDataFolder', () async {
    late http.BaseRequest seenReq;
    late String body;
    final client = MockClient((req) async {
      seenReq = req;
      body = utf8.decode(req.bodyBytes);
      return json({
        'id': 'n1',
        'name': 'ops-d1-1.json',
        'modifiedTime': '2026-10-15T00:00:00.000Z',
        'size': '2',
      });
    });
    final f = await GoogleDriveStore.withClient(
      client,
    ).create('ops-d1-1.json', utf8.encode('[]'), 'application/json');
    expect(seenReq.method, 'POST');
    expect(seenReq.url.path, '/upload/drive/v3/files');
    expect(seenReq.url.queryParameters['uploadType'], 'multipart');
    expect(seenReq.headers['content-type'], startsWith('multipart/related'));
    expect(body, contains('"parents":["appDataFolder"]'));
    expect(body, contains('"name":"ops-d1-1.json"'));
    expect(f.id, 'n1');
    expect(f.size, 2);
  });

  test('401 is authExpired', () async {
    final client = MockClient(
      (_) async => json({
        'error': {'code': 401, 'message': 'Invalid Credentials'},
      }, 401),
    );
    await expectLater(
      GoogleDriveStore.withClient(client).list(),
      throwsA(storageError(StorageErrorKind.authExpired)),
    );
  });

  test('429 and 5xx are retryable', () async {
    for (final code in [429, 500, 503]) {
      final client = MockClient(
        (_) async => json({
          'error': {'code': code, 'message': 'x'},
        }, code),
      );
      await expectLater(
        GoogleDriveStore.withClient(client).list(),
        throwsA(storageError(StorageErrorKind.retryable)),
        reason: '$code',
      );
    }
  });

  test('403 rate limit is retryable, other 403 is not', () async {
    Future<Object?> run(String reason) async {
      final client = MockClient(
        (_) async => json({
          'error': {
            'code': 403,
            'message': 'x',
            'errors': [
              {'reason': reason, 'message': 'x', 'domain': 'usageLimits'},
            ],
          },
        }, 403),
      );
      try {
        await GoogleDriveStore.withClient(client).list();
      } on StorageError catch (e) {
        return e.kind;
      }
      return null;
    }

    expect(await run('userRateLimitExceeded'), StorageErrorKind.retryable);
    expect(await run('forbidden'), StorageErrorKind.other);
  });

  test('a dropped connection is retryable', () async {
    final client = MockClient((_) async => throw http.ClientException('boom'));
    await expectLater(
      GoogleDriveStore.withClient(client).list(),
      throwsA(storageError(StorageErrorKind.retryable)),
    );
  });

  test(
    'a 401 with www-authenticate through AuthenticatedClient is authExpired',
    () async {
      final base = MockClient(
        (_) async => http.Response(
          '{"error":{"code":401}}',
          401,
          headers: {
            'www-authenticate': 'Bearer realm="x", error="invalid_token"',
          },
        ),
      );
      final creds = auth.AccessCredentials(
        auth.AccessToken(
          'Bearer',
          'tok',
          DateTime.now().toUtc().add(const Duration(hours: 1)),
        ),
        null,
        const ['scope'],
      );
      final client = auth.authenticatedClient(base, creds);
      await expectLater(
        GoogleDriveStore.withClient(client).list(),
        throwsA(storageError(StorageErrorKind.authExpired)),
      );
    },
  );

  test('list asks for pageSize 1000', () async {
    final client = MockClient((req) async {
      expect(req.url.queryParameters['pageSize'], '1000');
      return json({'files': []});
    });
    expect(await GoogleDriveStore.withClient(client).list(), isEmpty);
  });

  test('a file without id or modifiedTime is an error', () async {
    for (final f in [
      {'name': 'a', 'modifiedTime': '2026-10-14T03:00:00.000Z'},
      {'id': 'a', 'name': 'a'},
    ]) {
      final client = MockClient(
        (_) async => json({
          'files': [f],
        }),
      );
      await expectLater(
        GoogleDriveStore.withClient(client).list(),
        throwsA(storageError(StorageErrorKind.other)),
        reason: '$f',
      );
    }
  });

  test('an unparseable body is retryable', () async {
    final client = MockClient(
      (_) async => http.Response(
        '{not json',
        200,
        headers: {'content-type': 'application/json; charset=utf-8'},
      ),
    );
    await expectLater(
      GoogleDriveStore.withClient(client).list(),
      throwsA(storageError(StorageErrorKind.retryable)),
    );
  });

  test('a request that never completes times out as retryable', () async {
    final client = MockClient((_) => Completer<http.Response>().future);
    final store = GoogleDriveStore.withClient(
      client,
      connectTimeout: const Duration(milliseconds: 20),
      requestTimeout: const Duration(milliseconds: 50),
    );
    await expectLater(
      store.list(),
      throwsA(storageError(StorageErrorKind.retryable)),
    );
  });

  test('a body that stalls hits the whole-request timeout', () async {
    final client = MockClient.streaming(
      (req, _) async => http.StreamedResponse(
        StreamController<List<int>>().stream,
        200,
        headers: {'content-type': 'application/octet-stream'},
      ),
    );
    final store = GoogleDriveStore.withClient(
      client,
      connectTimeout: const Duration(milliseconds: 500),
      requestTimeout: const Duration(milliseconds: 50),
    );
    await expectLater(
      store.download(
        const RemoteFile(id: 'a', name: 'x', modifiedTime: '', size: 1),
      ),
      throwsA(storageError(StorageErrorKind.retryable)),
    );
  });
}

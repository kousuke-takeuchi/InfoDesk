import 'dart:async';
import 'dart:io';

import 'package:googleapis/drive/v3.dart' as drive;
import 'package:googleapis_auth/googleapis_auth.dart' as auth;
import 'package:http/http.dart' as http;

import 'drive_store.dart';
import 'timeouts.dart';

const _fields = 'id,name,modifiedTime,size';

/// Google Drive appDataFolder. Sign-in is not handled here: pass a client that
/// is already authenticated (see `extension_google_sign_in_as_googleapis_auth`).
class GoogleDriveStore implements DriveStore {
  GoogleDriveStore(this._api, {this.requestTimeout = defaultRequestTimeout});

  /// Wraps an authenticated (or, in tests, mocked) [http.Client]. Response
  /// headers must arrive within [connectTimeout], and the whole request
  /// (including the body read) within [requestTimeout].
  GoogleDriveStore.withClient(
    http.Client client, {
    Duration connectTimeout = defaultConnectTimeout,
    this.requestTimeout = defaultRequestTimeout,
  }) : _api = drive.DriveApi(TimeoutClient(client, connectTimeout));

  final drive.DriveApi _api;
  final Duration requestTimeout;

  @override
  Future<List<RemoteFile>> list() => _guard(() async {
    final out = <RemoteFile>[];
    String? page;
    do {
      final r = await _api.files.list(
        spaces: 'appDataFolder',
        pageSize: 1000,
        pageToken: page,
        $fields: 'nextPageToken,files($_fields)',
      );
      for (final f in r.files ?? const <drive.File>[]) {
        out.add(_toRemote(f));
      }
      page = r.nextPageToken;
    } while (page != null && page.isNotEmpty);
    return out;
  });

  @override
  Future<List<int>> download(RemoteFile f) => _guard(() async {
    final media =
        await _api.files.get(
              f.id,
              downloadOptions: drive.DownloadOptions.fullMedia,
            )
            as drive.Media;
    final bytes = <int>[];
    await for (final chunk in media.stream) {
      bytes.addAll(chunk);
    }
    return bytes;
  });

  @override
  Future<RemoteFile> create(String name, List<int> bytes, String mime) =>
      _guard(() async {
        final meta = drive.File()
          ..name = name
          ..parents = ['appDataFolder'];
        final created = await _api.files.create(
          meta,
          $fields: _fields,
          uploadMedia: drive.Media(
            Stream.value(bytes),
            bytes.length,
            contentType: mime,
          ),
        );
        return _toRemote(created);
      });

  RemoteFile _toRemote(drive.File f) {
    final id = f.id;
    final modified = f.modifiedTime;
    if (id == null || id.isEmpty || modified == null) {
      throw const StorageError(
        StorageErrorKind.other,
        '保存先のエラー: ファイルの情報が不完全です',
      );
    }
    return RemoteFile(
      id: id,
      name: f.name ?? '',
      modifiedTime: rfc3339Millis(modified),
      size: int.tryParse(f.size ?? ''),
    );
  }

  Future<T> _guard<T>(Future<T> Function() run) async {
    try {
      return await run().timeout(requestTimeout);
    } on StorageError {
      rethrow;
    } on auth.AccessDeniedException {
      // AuthenticatedClient throws this for any response carrying a
      // `www-authenticate` header, which is how Google answers a 401.
      throw const StorageError(StorageErrorKind.authExpired, reauthMessage);
    } on drive.DetailedApiRequestError catch (e) {
      final code = e.status ?? 0;
      if (code == 401) {
        throw const StorageError(StorageErrorKind.authExpired, reauthMessage);
      }
      // Retryable beyond the usual 429/5xx: 408 (request timeout) and the 403
      // reasons in [_isRateLimit], including `backendError`, which Drive sends
      // as a transient server-side failure.
      if (code == 408 ||
          code == 429 ||
          code >= 500 ||
          (code == 403 && _isRateLimit(e))) {
        throw const StorageError(StorageErrorKind.retryable, networkMessage);
      }
      throw StorageError(StorageErrorKind.other, '保存先のエラー: HTTP $code');
    } on drive.ApiRequestError catch (e) {
      throw StorageError(StorageErrorKind.other, '保存先のエラー: ${e.message}');
    } on http.ClientException {
      throw const StorageError(StorageErrorKind.retryable, networkMessage);
    } on FormatException {
      // A truncated or garbled body: treat like a dropped connection.
      throw const StorageError(StorageErrorKind.retryable, networkMessage);
    } on SocketException {
      throw const StorageError(StorageErrorKind.retryable, networkMessage);
    } on TimeoutException {
      throw const StorageError(StorageErrorKind.retryable, networkMessage);
    }
  }
}

/// Drive reports millisecond times; keep them as sent.
String rfc3339Millis(DateTime t) => t.toUtc().toIso8601String();

bool _isRateLimit(drive.DetailedApiRequestError e) => e.errors.any(
  (d) => const {
    'rateLimitExceeded',
    'userRateLimitExceeded',
    'backendError',
  }.contains(d.reason),
);

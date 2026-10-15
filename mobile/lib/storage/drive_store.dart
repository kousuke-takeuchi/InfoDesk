/// Storage abstraction for the sync folder. The phone only reads, plus creates
/// its own `ops-*` files, so there is no update or delete.
///
/// Stores return every file they see (NAS: safe plain names only). Dropping
/// names that `parseFileName` does not recognise is the sync pass's job.
library;

class RemoteFile {
  const RemoteFile({
    required this.id,
    required this.name,
    required this.modifiedTime,
    required this.size,
  });

  final String id;
  final String name;

  /// RFC 3339, UTC.
  final String modifiedTime;
  final int? size;
}

enum StorageErrorKind { retryable, authExpired, badCredentials, other }

/// A storage failure. [message] is Japanese and fit for the screen.
class StorageError implements Exception {
  const StorageError(this.kind, this.message);

  final StorageErrorKind kind;
  final String message;

  @override
  String toString() => message;
}

const networkMessage = 'ネットワークに接続できません。次の同期で再試行します。';
const reauthMessage = 'Google の再ログインが必要です';

abstract class DriveStore {
  Future<List<RemoteFile>> list();
  Future<List<int>> download(RemoteFile f);
  Future<RemoteFile> create(String name, List<int> bytes, String mime);
}

/// In-memory store for tests.
class MemoryDrive implements DriveStore {
  final _files = <String, ({RemoteFile meta, List<int> bytes})>{};
  StorageError? _fail;
  int _next = 0;

  /// Upsert: adds a file, or replaces the content of the one with the same
  /// name. Test setup helper; [create] does not behave this way.
  RemoteFile put(String name, List<int> bytes, {DateTime? modified}) {
    final existing = _files.values
        .where((e) => e.meta.name == name)
        .map((e) => e.meta.id)
        .firstOrNull;
    final id = existing ?? 'm${_next++}';
    final meta = RemoteFile(
      id: id,
      name: name,
      modifiedTime: _rfc3339(modified ?? DateTime.now().toUtc()),
      size: bytes.length,
    );
    _files[id] = (meta: meta, bytes: List<int>.unmodifiable(bytes));
    return meta;
  }

  /// The next call (any method) throws [error], once.
  void failNext(StorageError error) => _fail = error;

  List<String> get names => _files.values.map((e) => e.meta.name).toList();

  void _check() {
    final f = _fail;
    if (f != null) {
      _fail = null;
      throw f;
    }
  }

  @override
  Future<List<RemoteFile>> list() async {
    _check();
    return _files.values.map((e) => e.meta).toList();
  }

  @override
  Future<List<int>> download(RemoteFile f) async {
    _check();
    final e = _files[f.id];
    if (e == null) {
      throw const StorageError(StorageErrorKind.other, '保存先のエラー: ファイルがありません');
    }
    return e.bytes;
  }

  @override
  /// Like Drive, always adds a new file, even when the name already exists.
  Future<RemoteFile> create(String name, List<int> bytes, String mime) async {
    _check();
    final id = 'm${_next++}';
    final meta = RemoteFile(
      id: id,
      name: name,
      modifiedTime: _rfc3339(DateTime.now().toUtc()),
      size: bytes.length,
    );
    _files[id] = (meta: meta, bytes: List<int>.unmodifiable(bytes));
    return meta;
  }
}

String _rfc3339(DateTime t) {
  final u = t.toUtc();
  String p(int n, [int w = 2]) => n.toString().padLeft(w, '0');
  return '${p(u.year, 4)}-${p(u.month)}-${p(u.day)}T'
      '${p(u.hour)}:${p(u.minute)}:${p(u.second)}Z';
}

/// Shared with the NAS store.
String rfc3339Seconds(DateTime t) => _rfc3339(t);

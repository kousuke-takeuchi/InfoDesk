import 'dart:convert';

import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/data/local_store.dart';
import 'package:infodesk_mobile/storage/drive_store.dart';
import 'package:infodesk_mobile/sync/engine.dart';
import 'package:infodesk_mobile/sync/format.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';

ItemRecord item(int id) => ItemRecord(
  id: id,
  url: 'https://example.com/$id',
  title: 'title $id',
  excerpt: 'excerpt',
  aiSummary: null,
  imageUrl: null,
  sourceId: 1,
  sourceName: 'src',
  category: 'tech',
  score: null,
  publishedAt: null,
  fetchedAt: '2026-10-10T00:00:00Z',
  topicIds: const [],
);

String sha(List<int> b) => sha256.convert(b).toString();

/// What the Mac would publish.
class Mac {
  Mac(this.drive);
  final MemoryDrive drive;
  final files = <String, List<int>>{};
  String dbId = 'db1';

  void put(String name, List<int> bytes) {
    files[name] = bytes;
    drive.put(name, bytes);
  }

  void day(String date, List<int> ids) => put(
    'day-$date.json.gz',
    gzipJson(
      DayFile(date: date, items: [for (final i in ids) item(i)]).toJson(),
    ),
  );

  void meta() => put(
    'meta.json.gz',
    gzipJson(
      MetaFile(
        categories: [CategoryMeta(id: 'tech', label: 'Tech')],
        topics: [],
        sources: [],
      ).toJson(),
    ),
  );

  void digest() => put(
    'digest-day-2026-10-10.json.gz',
    gzipJson(
      DigestFile(
        period: 'day',
        periodStart: '2026-10-10',
        stats: null,
        entries: [
          DigestRecord(
            scope: null,
            body: 'hello',
            status: 'ok',
            itemIds: [1],
            provider: 'p',
            createdAt: '2026-10-10T00:00:00Z',
          ),
        ],
      ).toJson(),
    ),
  );

  void snapshot(Map<int, ItemState> state) => put(
    'state-snapshot.json.gz',
    gzipJson(StateSnapshot(covers: [], state: state).toJson()),
  );

  void ops(String device, String stamp, List<Op> ops) => put(
    'ops-$device-${stamp}Z-0001.json',
    utf8.encode(jsonEncode(OpsFile(device: device, ops: ops).toJson())),
  );

  /// Writes the manifest last, over every data file.
  void manifest({Map<String, String>? overrideSha}) {
    final entries = <String, FileEntry>{};
    for (final e in files.entries) {
      final k = parseFileName(e.key)!.kind;
      if (k == FileKindType.ops || k == FileKindType.manifest) continue;
      entries[e.key] = FileEntry(
        sha256: overrideSha?[e.key] ?? sha(e.value),
        size: e.value.length,
        updatedAt: '2026-10-10T00:00:00Z',
      );
    }
    final m = Manifest(
      schema: 1,
      dbId: dbId,
      writtenAt: '2026-10-10T00:00:00Z',
      writer: 'mac',
      files: entries,
    );
    files['manifest.json'] = utf8.encode(jsonEncode(m.toJson()));
    drive.put('manifest.json', files['manifest.json']!);
  }
}

/// Counts downloads.
class CountingDrive implements DriveStore {
  CountingDrive(this.inner);
  final MemoryDrive inner;
  final downloads = <String>[];
  @override
  Future<List<RemoteFile>> list() => inner.list();
  @override
  Future<List<int>> download(RemoteFile f) {
    downloads.add(f.name);
    return inner.download(f);
  }

  @override
  Future<RemoteFile> create(String name, List<int> bytes, String mime) =>
      inner.create(name, bytes, mime);
}

Op op(int id, Field f, bool v, String at, String dev) =>
    Op(itemId: id, field: f, value: v, at: at, device: dev);

void main() {
  sqfliteFfiInit();
  late LocalStore store;
  late MemoryDrive mem;
  late CountingDrive drive;
  late Mac mac;
  late SyncEngine engine;
  final now = DateTime.utc(2026, 10, 11, 9);

  setUp(() async {
    store = await LocalStore.inMemory(factory: databaseFactoryFfi);
    mem = MemoryDrive();
    drive = CountingDrive(mem);
    mac = Mac(mem);
    engine = SyncEngine(store, drive);
  });
  tearDown(() => store.close());

  void publish() {
    mac.meta();
    mac.day('2026-10-10', [1, 2]);
    mac.day('2026-10-09', [3]);
    mac.digest();
    mac.snapshot({
      1: ItemState(
        read: Stamp(value: true, at: '2026-10-10T01:00:00.000Z', device: 'mac'),
      ),
    });
    mac.manifest();
  }

  Future<StoredItem> row(int id) async =>
      (await store.items()).firstWhere((i) => i.record.id == id);

  List<String> opsFiles() =>
      mem.names.where((n) => n.startsWith('ops-')).toList();

  test('first sync imports days, digests, meta and the snapshot', () async {
    publish();
    final r = await engine.runOnce(now: now);
    expect(r.downloaded, 5);
    expect(r.changed, isTrue);
    expect(r.warnings, isEmpty);
    expect((await store.items()).length, 3);
    expect((await row(1)).isRead, isTrue);
    expect(await store.digests('day', '2026-10-10'), isNotNull);
    expect(await store.meta(), isNotNull);
    expect(await store.dbId(), 'db1');
  });

  test('a second pass with no changes downloads nothing', () async {
    publish();
    await engine.runOnce(now: now);
    drive.downloads.clear();
    final r = await engine.runOnce(now: now);
    expect(drive.downloads, ['manifest.json']);
    expect(r.downloaded, 0);
    expect(r.changed, isFalse);
  });

  test('a changed day re-downloads only that file', () async {
    publish();
    await engine.runOnce(now: now);
    mac.day('2026-10-10', [1, 2, 4]);
    mac.manifest();
    drive.downloads.clear();
    final r = await engine.runOnce(now: now);
    expect(drive.downloads, ['manifest.json', 'day-2026-10-10.json.gz']);
    expect(r.downloaded, 1);
    expect((await store.items()).length, 4);
  });

  test('phone ops upload once', () async {
    publish();
    await engine.runOnce(now: now);
    await store.setStarred(2, true, await store.deviceId(), now: now);
    final r = await engine.runOnce(now: now);
    expect(r.uploadedOps, 1);
    expect(opsFiles().length, 1);
    final dev = await store.deviceId();
    expect(opsFiles().single, opsFileName(dev, now, 1));
    final body = jsonDecode(
      utf8.decode(
        await mem.download(
          (await mem.list()).firstWhere((f) => f.name == opsFiles().single),
        ),
      ),
    );
    expect(body['device'], dev);
    expect((body['ops'] as List).length, 1);
    expect(await store.pendingOps(), isEmpty);
    final r2 = await engine.runOnce(now: now);
    expect(r2.uploadedOps, 0);
    expect(opsFiles().length, 1);
  });

  test('ops from another device apply', () async {
    publish();
    mac.ops('other', '20261010T020000000', [
      op(2, Field.starred, true, '2026-10-10T02:00:00.000Z', 'other'),
    ]);
    final r = await engine.runOnce(now: now);
    expect(r.appliedOps, 1);
    expect((await row(2)).isStarred, isTrue);
    expect(await store.pendingOps(), isEmpty);
    drive.downloads.clear();
    await engine.runOnce(now: now);
    expect(drive.downloads, ['manifest.json']);
  });

  test('after a local wipe the own uploaded ops come back', () async {
    publish();
    await engine.runOnce(now: now);
    final dev = await store.deviceId();
    await store.setStarred(2, true, dev, now: now);
    await engine.runOnce(now: now);
    expect(opsFiles().length, 1);
    await store.clearLocalCopy();
    expect(await store.deviceId(), dev);
    final r = await engine.runOnce(now: now);
    expect(r.appliedOps, 1);
    expect((await row(2)).isStarred, isTrue);
    expect(await store.pendingOps(), isEmpty);
    expect(r.uploadedOps, 0);
  });

  test(
    'a normal second pass does not download the own uploaded file',
    () async {
      publish();
      await engine.runOnce(now: now);
      await store.setStarred(2, true, await store.deviceId(), now: now);
      await engine.runOnce(now: now);
      drive.downloads.clear();
      await engine.runOnce(now: now);
      expect(drive.downloads, ['manifest.json']);
    },
  );

  test('a store write failure propagates and records nothing', () async {
    publish();
    final bad = _ThrowingDayStore(store);
    await expectLater(
      SyncEngine(bad, drive).runOnce(now: now),
      throwsA(isA<StateError>()),
    );
    expect(await store.syncedFile('day-2026-10-10.json.gz'), isNull);
  });

  test('unparsable ops file warns once and is not retried', () async {
    publish();
    mac.put('ops-other-20261010T020000000Z-0001.json', utf8.encode('{nope'));
    final r = await engine.runOnce(now: now);
    expect(r.warnings.length, 1);
    drive.downloads.clear();
    final r2 = await engine.runOnce(now: now);
    expect(r2.warnings, isEmpty);
    expect(drive.downloads, ['manifest.json']);
  });

  test('a db_id change resets and drops unsent ops', () async {
    publish();
    await engine.runOnce(now: now);
    final dev = await store.deviceId();
    await store.setRead(2, true, dev, now: now);
    mac.dbId = 'db2';
    mac.day('2026-10-10', [7]);
    mac.manifest();
    final r = await engine.runOnce(now: now);
    expect(await store.dbId(), 'db2');
    expect(r.uploadedOps, 0);
    expect(opsFiles(), isEmpty);
    expect((await store.items()).map((i) => i.record.id).toSet(), {3, 7});
  });

  test(
    'a failure mid-pass leaves state consistent and the next pass finishes',
    () async {
      publish();
      // list, manifest, then the first data file fails.
      var calls = 0;
      final flaky = _FailingOn(mem, () => ++calls == 3);
      final e = SyncEngine(store, flaky);
      await expectLater(e.runOnce(now: now), throwsA(isA<StorageError>()));
      expect(await store.syncedFile('state-snapshot.json.gz'), isNull);
      final r = await engine.runOnce(now: now);
      expect(r.warnings, isEmpty);
      expect((await store.items()).length, 3);
      expect((await row(1)).isRead, isTrue);
      expect(r.downloaded, 5);
    },
  );

  test('failNext on the upload keeps the ops pending', () async {
    publish();
    await engine.runOnce(now: now);
    await store.setStarred(2, true, await store.deviceId(), now: now);
    final flaky = _FailingOn(mem, () => false, failCreate: true);
    await expectLater(
      SyncEngine(store, flaky).runOnce(now: now),
      throwsA(isA<StorageError>()),
    );
    expect((await store.pendingOps()).length, 1);
    final r = await engine.runOnce(now: now);
    expect(r.uploadedOps, 1);
    expect(opsFiles().length, 1);
  });

  test('a sha mismatch warns and skips the file', () async {
    publish();
    mac.manifest(overrideSha: {'day-2026-10-09.json.gz': 'deadbeef'});
    final r = await engine.runOnce(now: now);
    expect(r.warnings.length, 1);
    expect(r.warnings.single, contains('day-2026-10-09.json.gz'));
    expect((await store.items()).length, 2);
    expect(await store.syncedFile('day-2026-10-09.json.gz'), isNull);
  });

  test('no manifest means nothing is uploaded', () async {
    mac.day('2026-10-10', [1]);
    await store.setStarred(1, true, 'x', now: now);
    final r = await engine.runOnce(now: now);
    expect(r.uploadedOps, 0);
    expect(r.warnings, isNotEmpty);
    expect(opsFiles(), isEmpty);
    expect(await store.dbId(), isNull);
  });

  test('two concurrent runs do not double-upload', () async {
    publish();
    await engine.runOnce(now: now);
    await store.setStarred(2, true, await store.deviceId(), now: now);
    final a = engine.runOnce(now: now);
    final b = engine.runOnce(now: now);
    await Future.wait([a, b]);
    expect(opsFiles().length, 1);
  });
}

class _FailingOn implements DriveStore {
  _FailingOn(this.inner, this.failNow, {this.failCreate = false});
  final MemoryDrive inner;
  final bool Function() failNow;
  final bool failCreate;
  Never _boom() => throw const StorageError(StorageErrorKind.retryable, 'net');
  @override
  Future<List<RemoteFile>> list() async {
    if (failNow()) _boom();
    return inner.list();
  }

  @override
  Future<List<int>> download(RemoteFile f) async {
    if (failNow()) _boom();
    return inner.download(f);
  }

  @override
  Future<RemoteFile> create(String name, List<int> bytes, String mime) async {
    if (failCreate) _boom();
    return inner.create(name, bytes, mime);
  }
}

/// A store whose day writes fail, like a full disk.
class _ThrowingDayStore implements LocalStore {
  _ThrowingDayStore(this.s);
  final LocalStore s;
  @override
  Future<void> upsertDay(DayFile day) async => throw StateError('disk full');
  @override
  Future<String?> dbId() => s.dbId();
  @override
  Future<void> resetForDbId(String id) => s.resetForDbId(id);
  @override
  Future<SyncedFile?> syncedFile(String name) => s.syncedFile(name);
  @override
  Future<void> putMeta(MetaFile m) => s.putMeta(m);
  @override
  Future<void> putSyncedFile(
    String name,
    String sha256,
    String kind, {
    bool imported = true,
  }) => s.putSyncedFile(name, sha256, kind, imported: imported);
  @override
  dynamic noSuchMethod(Invocation i) => super.noSuchMethod(i);
}

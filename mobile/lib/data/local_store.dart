import 'dart:convert';
import 'dart:math';

import 'package:sqflite/sqflite.dart';

import '../sync/format.dart';

/// The phone's local copy: items, digests, meta, read/star state, the journal
/// of own operations not yet written out, and the record of synced files.
///
/// Read and star state lives in its own table (`item_state`), keyed by the
/// Mac's item id and independent of `items`. That keeps marks when a day file
/// is replaced, lets a snapshot or op arrive before its day file, and means a
/// snapshot that omits old items (past the 60 day window) cannot erase them.

/// Query filter for [LocalStore.items] and [LocalStore.markAllRead].
/// `periodStart` and `periodEnd` are inclusive `YYYY-MM-DD` day-file dates.
class ItemFilter {
  const ItemFilter({
    this.periodStart,
    this.periodEnd,
    this.category,
    this.topicId,
    this.unreadOnly = false,
    this.starredOnly = false,
    this.search,
  });
  final String? periodStart;
  final String? periodEnd;
  final String? category;
  final int? topicId;
  final bool unreadOnly;
  final bool starredOnly;
  final String? search;
}

/// An item with its local state. A missing stamp means false.
class StoredItem {
  StoredItem({
    required this.record,
    required this.date,
    this.read,
    this.starred,
  });
  final ItemRecord record;

  /// The day file the item came from.
  final String date;
  final Stamp? read;
  final Stamp? starred;
  bool get isRead => read?.value ?? false;
  bool get isStarred => starred?.value ?? false;
}

/// An own operation waiting to be written to an `ops-*` file.
class PendingOp {
  PendingOp({required this.id, required this.op});
  final int id;
  final Op op;
  int get itemId => op.itemId;
  Field get field => op.field;
  bool get value => op.value;
  String get at => op.at;
  String get device => op.device;
}

class SyncedFile {
  SyncedFile({
    required this.name,
    required this.sha256,
    required this.kind,
    required this.imported,
  });
  final String name;
  final String sha256;
  final String kind;

  /// True once the file's content has been applied to the store.
  final bool imported;
}

class LocalStore {
  LocalStore._(this._db);
  final Database _db;

  static const _version = 1;

  /// Opens (creating if needed) the database at [path]. Pass [factory] to use
  /// something other than sqflite's default, e.g. `databaseFactoryFfi` in
  /// host tests.
  static Future<LocalStore> open(
    String path, {
    DatabaseFactory? factory,
  }) async {
    final f = factory ?? databaseFactory;
    final db = await f.openDatabase(
      path,
      options: OpenDatabaseOptions(
        version: _version,
        onCreate: _create,
        onUpgrade: _upgrade,
      ),
    );
    return LocalStore._(db);
  }

  static Future<LocalStore> inMemory({DatabaseFactory? factory}) =>
      open(inMemoryDatabasePath, factory: factory);

  Future<void> close() => _db.close();

  /// Migrations run here, one `if` per schema version, oldest first:
  ///
  ///     if (oldVersion < 2) { await db.execute('ALTER TABLE ...'); }
  ///
  /// Bump [_version] and make [_create] build the newest schema directly.
  static Future<void> _upgrade(
    Database db,
    int oldVersion,
    int newVersion,
  ) async {}

  static Future<void> _create(Database db, int version) async {
    await db.execute('''
CREATE TABLE items (
  id INTEGER PRIMARY KEY,
  date TEXT NOT NULL,
  url TEXT NOT NULL,
  title TEXT NOT NULL,
  excerpt TEXT NOT NULL,
  ai_summary TEXT,
  image_url TEXT,
  source_id INTEGER,
  source_name TEXT NOT NULL,
  category TEXT NOT NULL,
  score INTEGER,
  published_at TEXT,
  fetched_at TEXT NOT NULL
)''');
    await db.execute('CREATE INDEX items_date ON items(date)');
    await db.execute(
      'CREATE INDEX items_fetched ON items(fetched_at DESC, id DESC)',
    );
    await db.execute('CREATE INDEX items_category ON items(category)');
    await db.execute('''
CREATE TABLE item_topics (
  item_id INTEGER NOT NULL,
  topic_id INTEGER NOT NULL,
  PRIMARY KEY (item_id, topic_id)
)''');
    await db.execute('CREATE INDEX item_topics_topic ON item_topics(topic_id)');
    await db.execute('''
CREATE TABLE item_state (
  item_id INTEGER PRIMARY KEY,
  read INTEGER, read_at TEXT, read_device TEXT,
  starred INTEGER, starred_at TEXT, starred_device TEXT
)''');
    await db.execute('''
CREATE TABLE digests (
  period TEXT NOT NULL,
  period_start TEXT NOT NULL,
  body TEXT NOT NULL,
  PRIMARY KEY (period, period_start)
)''');
    await db.execute(
      'CREATE TABLE meta (id INTEGER PRIMARY KEY CHECK (id = 1), body TEXT NOT NULL)',
    );
    await db.execute('''
CREATE TABLE synced_files (
  name TEXT PRIMARY KEY,
  sha256 TEXT NOT NULL,
  kind TEXT NOT NULL,
  imported INTEGER NOT NULL
)''');
    await db.execute('''
CREATE TABLE pending_ops (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id INTEGER NOT NULL,
  field TEXT NOT NULL,
  value INTEGER NOT NULL,
  at TEXT NOT NULL,
  device TEXT NOT NULL,
  sent INTEGER NOT NULL DEFAULT 0
)''');
    await db.execute(
      'CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)',
    );
  }

  // ---- upserts ----

  /// Replaces the items of [day].date with the file's items. Read and star
  /// state is kept (it lives in `item_state`).
  Future<void> upsertDay(DayFile day) => _db.transaction((txn) async {
    final old = await txn.query(
      'items',
      columns: ['id'],
      where: 'date = ?',
      whereArgs: [day.date],
    );
    for (final r in old) {
      await txn.delete(
        'item_topics',
        where: 'item_id = ?',
        whereArgs: [r['id']],
      );
    }
    await txn.delete('items', where: 'date = ?', whereArgs: [day.date]);
    for (final i in day.items) {
      // An id moving between days must not leave a stale copy behind.
      await txn.delete('item_topics', where: 'item_id = ?', whereArgs: [i.id]);
      await txn.insert('items', {
        'id': i.id,
        'date': day.date,
        'url': i.url,
        'title': i.title,
        'excerpt': i.excerpt,
        'ai_summary': i.aiSummary,
        'image_url': i.imageUrl,
        'source_id': i.sourceId,
        'source_name': i.sourceName,
        'category': i.category,
        'score': i.score,
        'published_at': i.publishedAt,
        'fetched_at': i.fetchedAt,
      }, conflictAlgorithm: ConflictAlgorithm.replace);
      for (final t in i.topicIds.toSet()) {
        await txn.insert('item_topics', {'item_id': i.id, 'topic_id': t});
      }
    }
  });

  Future<void> upsertDigests(DigestFile d) => _db.insert('digests', {
    'period': d.period,
    'period_start': d.periodStart,
    'body': jsonEncode(d.toJson()),
  }, conflictAlgorithm: ConflictAlgorithm.replace);

  Future<void> putMeta(MetaFile m) => _db.insert('meta', {
    'id': 1,
    'body': jsonEncode(m.toJson()),
  }, conflictAlgorithm: ConflictAlgorithm.replace);

  // ---- queries ----

  (String, List<Object?>) _where(ItemFilter f) {
    final w = <String>[];
    final a = <Object?>[];
    if (f.periodStart != null) {
      w.add('i.date >= ?');
      a.add(f.periodStart);
    }
    if (f.periodEnd != null) {
      w.add('i.date <= ?');
      a.add(f.periodEnd);
    }
    if (f.category != null) {
      w.add('i.category = ?');
      a.add(f.category);
    }
    if (f.topicId != null) {
      w.add(
        'EXISTS (SELECT 1 FROM item_topics t WHERE t.item_id = i.id AND t.topic_id = ?)',
      );
      a.add(f.topicId);
    }
    if (f.unreadOnly) w.add('COALESCE(s.read, 0) = 0');
    if (f.starredOnly) w.add('COALESCE(s.starred, 0) = 1');
    final q = f.search?.trim();
    if (q != null && q.isNotEmpty) {
      final like =
          '%${q.replaceAll(r'\', r'\\').replaceAll('%', r'\%').replaceAll('_', r'\_')}%';
      w.add(
        r"(i.title LIKE ? ESCAPE '\' OR i.excerpt LIKE ? ESCAPE '\' "
        r"OR i.ai_summary LIKE ? ESCAPE '\' OR i.source_name LIKE ? ESCAPE '\')",
      );
      a.addAll([like, like, like, like]);
    }
    return (w.isEmpty ? '' : 'WHERE ${w.join(' AND ')}', a);
  }

  static const _from =
      'FROM items i LEFT JOIN item_state s ON s.item_id = i.id';

  /// Items newest first (`fetched_at`, then id), with local state.
  Future<List<StoredItem>> items({
    ItemFilter filter = const ItemFilter(),
    int? limit,
    int? offset,
  }) async {
    final (where, args) = _where(filter);
    // SQLite needs a LIMIT before OFFSET; -1 means no limit.
    final lim = limit != null || offset != null
        ? 'LIMIT ${limit ?? -1} OFFSET ${offset ?? 0}'
        : '';
    final rows = await _db.rawQuery(
      'SELECT i.*, s.read, s.read_at, s.read_device, s.starred, '
      's.starred_at, s.starred_device $_from $where '
      'ORDER BY i.fetched_at DESC, i.id DESC $lim',
      args,
    );
    if (rows.isEmpty) return [];
    final ids = [for (final r in rows) r['id'] as int];
    final topics = <int, List<int>>{};
    // Chunked to stay under SQLite's variable limit.
    for (var k = 0; k < ids.length; k += 500) {
      final chunk = ids.sublist(k, k + 500 > ids.length ? ids.length : k + 500);
      final tr = await _db.rawQuery(
        'SELECT item_id, topic_id FROM item_topics WHERE item_id IN '
        '(${List.filled(chunk.length, '?').join(',')}) ORDER BY topic_id',
        chunk,
      );
      for (final t in tr) {
        topics
            .putIfAbsent(t['item_id'] as int, () => [])
            .add(t['topic_id'] as int);
      }
    }
    return [
      for (final r in rows)
        StoredItem(
          record: ItemRecord(
            id: r['id'] as int,
            url: r['url'] as String,
            title: r['title'] as String,
            excerpt: r['excerpt'] as String,
            aiSummary: r['ai_summary'] as String?,
            imageUrl: r['image_url'] as String?,
            sourceId: r['source_id'] as int?,
            sourceName: r['source_name'] as String,
            category: r['category'] as String,
            score: r['score'] as int?,
            publishedAt: r['published_at'] as String?,
            fetchedAt: r['fetched_at'] as String,
            topicIds: topics[r['id'] as int] ?? const [],
          ),
          date: r['date'] as String,
          read: _stamp(r['read'], r['read_at'], r['read_device']),
          starred: _stamp(r['starred'], r['starred_at'], r['starred_device']),
        ),
    ];
  }

  static Stamp? _stamp(Object? v, Object? at, Object? dev) => v == null
      ? null
      : Stamp(value: v == 1, at: at as String, device: dev as String);

  Future<DigestFile?> digests(String period, String start) async {
    final r = await _db.query(
      'digests',
      where: 'period = ? AND period_start = ?',
      whereArgs: [period, start],
    );
    return r.isEmpty
        ? null
        : DigestFile.fromJson(jsonDecode(r.first['body'] as String));
  }

  Future<MetaFile?> meta() async {
    final r = await _db.query('meta');
    return r.isEmpty
        ? null
        : MetaFile.fromJson(jsonDecode(r.first['body'] as String));
  }

  // ---- local changes ----

  Future<Stamp?> _current(DatabaseExecutor x, int id, Field f) async {
    final r = await x.query(
      'item_state',
      where: 'item_id = ?',
      whereArgs: [id],
    );
    if (r.isEmpty) return null;
    final c = f.json;
    return _stamp(r.first[c], r.first['${c}_at'], r.first['${c}_device']);
  }

  Future<void> _writeStamp(DatabaseExecutor x, int id, Field f, Stamp s) async {
    await x.insert('item_state', {
      'item_id': id,
    }, conflictAlgorithm: ConflictAlgorithm.ignore);
    await x.update(
      'item_state',
      {
        f.json: s.value ? 1 : 0,
        '${f.json}_at': s.at,
        '${f.json}_device': s.device,
      },
      where: 'item_id = ?',
      whereArgs: [id],
    );
  }

  /// Sets the field, stamping it with [now] and [device], and journals an op.
  /// A local change is written even if the clock is behind an older stamp.
  /// Returns false, changing and journaling nothing, if the value is already
  /// the current one (a missing stamp counts as false).
  Future<bool> _setLocal(
    DatabaseExecutor x,
    int id,
    Field f,
    bool value,
    String device,
    DateTime now,
  ) async {
    final cur = await _current(x, id, f);
    if ((cur?.value ?? false) == value) return false;
    final at = opTime(now);
    await _writeStamp(x, id, f, Stamp(value: value, at: at, device: device));
    await x.insert('pending_ops', {
      'item_id': id,
      'field': f.json,
      'value': value ? 1 : 0,
      'at': at,
      'device': device,
    });
    return true;
  }

  Future<bool> setRead(int id, bool value, String device, {DateTime? now}) =>
      _db.transaction(
        (txn) => _setLocal(
          txn,
          id,
          Field.read,
          value,
          device,
          now ?? DateTime.now(),
        ),
      );

  Future<bool> setStarred(int id, bool value, String device, {DateTime? now}) =>
      _db.transaction(
        (txn) => _setLocal(
          txn,
          id,
          Field.starred,
          value,
          device,
          now ?? DateTime.now(),
        ),
      );

  /// Marks every unread item matching [filter] as read. Returns how many
  /// changed. `unreadOnly` is implied.
  Future<int> markAllRead(ItemFilter filter, String device, {DateTime? now}) =>
      _db.transaction((txn) async {
        final (where, args) = _where(
          ItemFilter(
            periodStart: filter.periodStart,
            periodEnd: filter.periodEnd,
            category: filter.category,
            topicId: filter.topicId,
            unreadOnly: true,
            starredOnly: filter.starredOnly,
            search: filter.search,
          ),
        );
        final rows = await txn.rawQuery('SELECT i.id $_from $where', args);
        final t = now ?? DateTime.now();
        var n = 0;
        for (final r in rows) {
          if (await _setLocal(
            txn,
            r['id'] as int,
            Field.read,
            true,
            device,
            t,
          )) {
            n++;
          }
        }
        return n;
      });

  // ---- remote changes ----

  Future<void> _applyOps(DatabaseExecutor x, Iterable<Op> ops) async {
    for (final raw in ops) {
      final o = normalizeOp(raw);
      if (o == null) continue;
      final cur = await _current(x, o.itemId, o.field);
      final s = Stamp(value: o.value, at: o.at, device: o.device);
      if (cur == null || wins(s, cur)) {
        await _writeStamp(x, o.itemId, o.field, s);
      }
    }
  }

  /// Applies ops from other files in one transaction: each is normalized and
  /// applied only when it beats the current stamp. Nothing is journaled.
  /// Own ops are applied like any other, so there is no device parameter:
  /// applying them again is idempotent, and after a reset they may be the
  /// only copy of the state.
  Future<void> applyRemoteOps(List<Op> ops) =>
      _db.transaction((txn) => _applyOps(txn, ops));

  /// Applies a snapshot as a diff: each stamp it holds competes with the
  /// stored one under the same win rule. Items it does not mention are left
  /// alone. Nothing is journaled.
  Future<void> applySnapshot(StateSnapshot snap) {
    final ops = <Op>[];
    for (final e in snap.state.entries) {
      for (final (f, s) in [
        (Field.read, e.value.read),
        (Field.starred, e.value.starred),
      ]) {
        if (s != null) {
          ops.add(
            Op(
              itemId: e.key,
              field: f,
              value: s.value,
              at: s.at,
              device: s.device,
            ),
          );
        }
      }
    }
    return applyRemoteOps(ops);
  }

  // ---- pending ops ----

  /// Unsent own ops, oldest first.
  Future<List<PendingOp>> pendingOps() async {
    final r = await _db.query('pending_ops', where: 'sent = 0', orderBy: 'id');
    return [
      for (final x in r)
        PendingOp(
          id: x['id'] as int,
          op: Op(
            itemId: x['item_id'] as int,
            field: Field.parse(x['field'] as String),
            value: x['value'] == 1,
            at: x['at'] as String,
            device: x['device'] as String,
          ),
        ),
    ];
  }

  /// Marks the ops as written out. They are deleted: the journal only holds
  /// what is still to send, and state lives in `item_state`.
  Future<void> markOpsSent(List<int> ids) => _db.transaction((txn) async {
    for (final id in ids) {
      await txn.delete('pending_ops', where: 'id = ?', whereArgs: [id]);
    }
  });

  // ---- synced files ----

  Future<SyncedFile?> syncedFile(String name) async {
    final r = await _db.query(
      'synced_files',
      where: 'name = ?',
      whereArgs: [name],
    );
    if (r.isEmpty) return null;
    final x = r.first;
    return SyncedFile(
      name: name,
      sha256: x['sha256'] as String,
      kind: x['kind'] as String,
      imported: x['imported'] == 1,
    );
  }

  /// Records a file's hash. [imported] is false for a file that was only
  /// downloaded and true once its content is in the store.
  Future<void> putSyncedFile(
    String name,
    String sha256,
    String kind, {
    bool imported = true,
  }) => _db.insert('synced_files', {
    'name': name,
    'sha256': sha256,
    'kind': kind,
    'imported': imported ? 1 : 0,
  }, conflictAlgorithm: ConflictAlgorithm.replace);

  // ---- settings and db_id ----

  Future<String?> getSetting(String key) async {
    final r = await _db.query('settings', where: 'key = ?', whereArgs: [key]);
    return r.isEmpty ? null : r.first['value'] as String;
  }

  Future<void> setSetting(String key, String value) => _db.insert('settings', {
    'key': key,
    'value': value,
  }, conflictAlgorithm: ConflictAlgorithm.replace);

  Future<String?> dbId() => getSetting('db_id');

  static const _dataTables = [
    'items',
    'item_topics',
    'item_state',
    'digests',
    'meta',
    'synced_files',
    'pending_ops',
  ];

  /// Wipes the synced data (not settings, so the device id and credentials
  /// survive), then records [newId]. Used when `manifest.db_id` changes: old
  /// item ids no longer mean anything.
  Future<void> resetForDbId(String newId) => _db.transaction((txn) async {
    await _wipeData(txn);
    await txn.insert('settings', {
      'key': 'db_id',
      'value': newId,
    }, conflictAlgorithm: ConflictAlgorithm.replace);
  });

  /// Deletes the local copy of items, digests, state, meta, synced-file
  /// records and unsent ops. Settings (including `db_id` and the device id)
  /// are kept.
  Future<void> clearLocalCopy() => _db.transaction(_wipeData);

  /// For a switch to another storage: wipes the data and the synced-file
  /// records, but keeps the unsent ops, so reads and stars made on this phone
  /// still go out. Settings are kept.
  Future<void> clearForNewStorage() => _db.transaction((txn) async {
    for (final t in _dataTables) {
      if (t != 'pending_ops') await txn.delete(t);
    }
  });

  Future<void> _wipeData(DatabaseExecutor x) async {
    for (final t in _dataTables) {
      await x.delete(t);
    }
  }

  /// This install's id, a UUID v4 made once and kept in settings. It is used
  /// in `ops-<device>-...` file names, so it must stay ASCII.
  Future<String> deviceId() async {
    final cur = await getSetting('device_id');
    if (cur != null) return cur;
    final r = Random.secure();
    final b = List<int>.generate(16, (_) => r.nextInt(256));
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    final h = [for (final x in b) x.toRadixString(16).padLeft(2, '0')].join();
    final id =
        '${h.substring(0, 8)}-${h.substring(8, 12)}-${h.substring(12, 16)}-'
        '${h.substring(16, 20)}-${h.substring(20)}';
    await _db.insert('settings', {
      'key': 'device_id',
      'value': id,
    }, conflictAlgorithm: ConflictAlgorithm.ignore);
    return (await getSetting('device_id'))!;
  }
}

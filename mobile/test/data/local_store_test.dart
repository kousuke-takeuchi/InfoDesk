import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/data/local_store.dart';
import 'package:infodesk_mobile/sync/format.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';

ItemRecord item(
  int id, {
  String category = 'tech',
  List<int> topics = const [],
  String title = 'title',
  String excerpt = 'excerpt',
  String fetchedAt = '2026-10-10T00:00:00Z',
}) => ItemRecord(
  id: id,
  url: 'https://example.com/$id',
  title: title,
  excerpt: excerpt,
  aiSummary: null,
  imageUrl: null,
  sourceId: 1,
  sourceName: 'src',
  category: category,
  score: null,
  publishedAt: null,
  fetchedAt: fetchedAt,
  topicIds: topics,
);

Op op(int id, Field f, bool v, String at, String dev) =>
    Op(itemId: id, field: f, value: v, at: at, device: dev);

DateTime t(int sec) => DateTime.utc(2026, 10, 10, 12, 0, sec);

void main() {
  sqfliteFfiInit();
  late LocalStore s;

  setUp(() async {
    s = await LocalStore.inMemory(factory: databaseFactoryFfi);
  });
  tearDown(() => s.close());

  Future<void> seed() => s.upsertDay(
    DayFile(
      date: '2026-10-10',
      items: [
        item(1, category: 'tech', topics: [10], title: 'Rust news'),
        item(2, category: 'news', topics: [10, 11], excerpt: 'about Dart'),
        item(3, category: 'tech', topics: [11]),
      ],
    ),
  );

  test('local journaling: only real changes create pending ops', () async {
    await seed();
    expect(await s.setRead(1, false, 'phone', now: t(1)), isFalse);
    expect(await s.pendingOps(), isEmpty);
    expect(await s.setRead(1, true, 'phone', now: t(2)), isTrue);
    expect(await s.setRead(1, true, 'phone', now: t(3)), isFalse);
    expect(await s.setStarred(1, true, 'phone', now: t(4)), isTrue);
    final ops = await s.pendingOps();
    expect(ops.map((o) => o.field), [Field.read, Field.starred]);
    expect(ops.first.at, '2026-10-10T12:00:02.000Z');
    expect(ops.first.device, 'phone');
    expect((await s.items()).firstWhere((i) => i.record.id == 1).isRead, true);
    await s.markOpsSent([for (final o in ops) o.id]);
    expect(await s.pendingOps(), isEmpty);
  });

  test('remote ops are not journaled; a losing op changes nothing', () async {
    await seed();
    await s.setRead(1, true, 'phone', now: t(10));
    final before = await s.pendingOps();
    await s.applyRemoteOps([
      op(1, Field.read, false, '2026-10-10T12:00:05Z', 'mac'),
    ]);
    final row = (await s.items()).firstWhere((i) => i.record.id == 1);
    expect(row.isRead, true);
    expect(row.read!.device, 'phone');
    expect((await s.pendingOps()).length, before.length);
    // A winning op with an offset timestamp is normalized and applied.
    await s.applyRemoteOps([
      op(1, Field.read, false, '2026-10-10T21:00:30+09:00', 'mac'),
    ]);
    final r2 = (await s.items()).firstWhere((i) => i.record.id == 1);
    expect(r2.isRead, false);
    expect(r2.read!.at, '2026-10-10T12:00:30.000Z');
    expect((await s.pendingOps()).length, before.length);
    // Unparsable ops are ignored.
    await s.applyRemoteOps([op(1, Field.read, true, 'garbage', 'mac')]);
    expect((await s.items()).firstWhere((i) => i.record.id == 1).isRead, false);
  });

  test(
    'snapshot is a diff: old items keep marks, newer local beats it',
    () async {
      await seed();
      await s.setStarred(1, true, 'phone', now: t(20)); // newer than snapshot
      await s.setRead(2, true, 'phone', now: t(1)); // not in snapshot at all
      // Item 99 is older than the snapshot window and absent from it.
      await s.upsertDay(DayFile(date: '2026-07-01', items: [item(99)]));
      await s.setRead(99, true, 'phone', now: t(2));
      final snap = StateSnapshot(
        covers: ['mac'],
        state: {
          1: ItemState(
            starred: Stamp(
              value: false,
              at: '2026-10-10T12:00:10.000Z',
              device: 'mac',
            ),
          ),
          3: ItemState(
            read: Stamp(
              value: true,
              at: '2026-10-10T12:00:10.000Z',
              device: 'mac',
            ),
          ),
        },
      );
      await s.applySnapshot(snap);
      final rows = {for (final i in await s.items()) i.record.id: i};
      expect(rows[1]!.isStarred, true);
      expect(rows[3]!.isRead, true);
      expect(rows[2]!.isRead, true);
      expect(rows[99]!.isRead, true);
      expect(await s.pendingOps(), hasLength(3));
    },
  );

  test('db_id reset wipes data but keeps settings', () async {
    await seed();
    await s.setRead(1, true, 'phone', now: t(1));
    await s.putSyncedFile('day-2026-10-10.json.gz', 'abc', 'day');
    await s.setSetting('k', 'v');
    await s.resetForDbId('new-id');
    expect(await s.dbId(), 'new-id');
    expect(await s.pendingOps(), isEmpty);
    expect(await s.items(), isEmpty);
    expect(await s.syncedFile('day-2026-10-10.json.gz'), isNull);
    expect(await s.getSetting('k'), 'v');
    // Old state does not come back for a reused item id.
    await s.upsertDay(DayFile(date: '2026-10-10', items: [item(1)]));
    expect((await s.items()).single.isRead, false);
    await s.resetForDbId('newer-id');
    expect(await s.dbId(), 'newer-id');
  });

  test('clearLocalCopy wipes data but keeps settings and db_id', () async {
    await seed();
    await s.setRead(1, true, 'phone', now: t(1));
    await s.resetForDbId('x');
    await s.upsertDay(DayFile(date: '2026-10-10', items: [item(1)]));
    await s.setSetting('k', 'v');
    await s.clearLocalCopy();
    expect(await s.items(), isEmpty);
    expect(await s.pendingOps(), isEmpty);
    expect(await s.getSetting('k'), 'v');
    expect(await s.dbId(), 'x');
  });

  test(
    'clearForNewStorage wipes data and synced files but keeps unsent ops',
    () async {
      await seed();
      await s.putSyncedFile('day-2026-10-10.json.gz', 'h', 'day');
      await s.setRead(1, true, 'phone', now: t(1));
      await s.setSetting('k', 'v');
      final before = await s.pendingOps();
      expect(before, isNotEmpty);
      await s.clearForNewStorage();
      expect(await s.items(), isEmpty);
      expect(await s.syncedFile('day-2026-10-10.json.gz'), isNull);
      final after = await s.pendingOps();
      expect(after.map((o) => o.op.itemId), before.map((o) => o.op.itemId));
      expect(await s.getSetting('k'), 'v');
    },
  );

  test('deviceId is a stable UUID v4 that survives resets', () async {
    final id = await s.deviceId();
    expect(
      id,
      matches(
        RegExp(
          r'^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$',
        ),
      ),
    );
    expect(await s.deviceId(), id);
    await s.resetForDbId('a');
    expect(await s.deviceId(), id);
    await s.clearLocalCopy();
    expect(await s.deviceId(), id);
  });

  test('search escapes % and _', () async {
    await s.upsertDay(
      DayFile(
        date: '2026-10-10',
        items: [
          item(1, title: '100% sure'),
          item(2, title: 'a_b'),
          item(3, title: 'axb and 100 plain'),
        ],
      ),
    );
    Future<List<int>> ids(String q) async => [
      for (final i in await s.items(filter: ItemFilter(search: q))) i.record.id,
    ];
    expect(await ids('%'), [1]);
    expect(await ids('_'), [2]);
    expect(await ids('a_b'), [2]);
  });

  test('an item moving between days leaves no stale copy', () async {
    await s.upsertDay(
      DayFile(
        date: '2026-10-09',
        items: [
          item(1, topics: [10]),
        ],
      ),
    );
    await s.upsertDay(
      DayFile(
        date: '2026-10-10',
        items: [
          item(1, topics: [11]),
        ],
      ),
    );
    var all = await s.items();
    expect(all, hasLength(1));
    expect(all.single.date, '2026-10-10');
    expect(all.single.record.topicIds, [11]);
    // The old day file arriving again without the item changes nothing.
    await s.upsertDay(DayFile(date: '2026-10-09', items: []));
    all = await s.items();
    expect(all, hasLength(1));
    expect(all.single.date, '2026-10-10');
  });

  test('snapshot ties on at resolve by device; at is normalized', () async {
    await seed();
    await s.applyRemoteOps([
      op(1, Field.read, true, '2026-10-10T12:00:10.000Z', 'b'),
    ]);
    StateSnapshot snap(String at, String dev, bool v) => StateSnapshot(
      covers: [],
      state: {
        1: ItemState(
          read: Stamp(value: v, at: at, device: dev),
        ),
      },
    );
    Future<StoredItem> one() async =>
        (await s.items()).firstWhere((i) => i.record.id == 1);
    // Smaller device loses the tie.
    await s.applySnapshot(snap('2026-10-10T12:00:10.000Z', 'a', false));
    expect((await one()).isRead, true);
    expect((await one()).read!.device, 'b');
    // Larger device wins the tie.
    await s.applySnapshot(snap('2026-10-10T12:00:10.000Z', 'c', false));
    expect((await one()).isRead, false);
    expect((await one()).read!.device, 'c');
    // A non-normalized at is normalized before comparing and storing.
    await s.applySnapshot(snap('2026-10-10T21:00:30+09:00', 'a', true));
    expect((await one()).isRead, true);
    expect((await one()).read!.at, '2026-10-10T12:00:30.000Z');
  });

  test('offset works without limit', () async {
    await s.upsertDay(
      DayFile(
        date: '2026-10-10',
        items: [
          item(1, fetchedAt: '2026-10-10T00:00:01Z'),
          item(2, fetchedAt: '2026-10-10T00:00:02Z'),
          item(3, fetchedAt: '2026-10-10T00:00:03Z'),
        ],
      ),
    );
    final r = await s.items(offset: 1);
    expect([for (final i in r) i.record.id], [2, 1]);
  });

  test('markAllRead journals each of several items', () async {
    await seed();
    final n = await s.markAllRead(const ItemFilter(), 'phone', now: t(5));
    expect(n, 3);
    final ops = await s.pendingOps();
    expect(ops.map((o) => o.itemId).toSet(), {1, 2, 3});
    expect(ops.every((o) => o.field == Field.read && o.value), isTrue);
  });

  test('upsertDay replaces the date but keeps read and star state', () async {
    await seed();
    await s.setRead(1, true, 'phone', now: t(1));
    await s.setStarred(2, true, 'phone', now: t(1));
    await s.upsertDay(
      DayFile(
        date: '2026-10-10',
        items: [
          item(1, title: 'new'),
          item(4),
        ],
      ),
    );
    final rows = {for (final i in await s.items()) i.record.id: i};
    expect(rows.keys.toSet(), {1, 4});
    expect(rows[1]!.record.title, 'new');
    expect(rows[1]!.isRead, true);
    // State of a dropped item survives in case it comes back.
    await s.upsertDay(DayFile(date: '2026-10-10', items: [item(2)]));
    expect((await s.items()).single.isStarred, true);
  });

  test('filters', () async {
    await seed();
    await s.upsertDay(
      DayFile(
        date: '2026-10-11',
        items: [item(5, category: 'news')],
      ),
    );
    await s.setRead(1, true, 'phone', now: t(1));
    await s.setStarred(3, true, 'phone', now: t(1));
    Future<List<int>> ids(ItemFilter f, {int? limit, int? offset}) async => [
      for (final i in await s.items(filter: f, limit: limit, offset: offset))
        i.record.id,
    ];
    expect(await ids(const ItemFilter()), hasLength(4));
    expect((await ids(const ItemFilter(unreadOnly: true))).toSet(), {2, 3, 5});
    expect(await ids(const ItemFilter(starredOnly: true)), [3]);
    expect((await ids(const ItemFilter(category: 'news'))).toSet(), {2, 5});
    expect((await ids(const ItemFilter(topicId: 10))).toSet(), {1, 2});
    expect(await ids(const ItemFilter(search: 'rust')), [1]);
    expect(await ids(const ItemFilter(search: 'dart')), [2]);
    expect(await ids(const ItemFilter(search: '100%')), isEmpty);
    expect(
      await ids(
        const ItemFilter(periodStart: '2026-10-11', periodEnd: '2026-10-11'),
      ),
      [5],
    );
    expect(await ids(const ItemFilter(), limit: 2), hasLength(2));
    expect(await ids(const ItemFilter(), limit: 2, offset: 3), hasLength(1));
    final one = (await s.items(
      filter: const ItemFilter(topicId: 11),
    )).firstWhere((i) => i.record.id == 2);
    expect(one.record.topicIds.toSet(), {10, 11});
  });

  test('markAllRead journals only the items it changed', () async {
    await seed();
    await s.setRead(1, true, 'phone', now: t(1));
    final n = await s.markAllRead(
      const ItemFilter(category: 'tech'),
      'phone',
      now: t(2),
    );
    expect(n, 1);
    expect(await s.pendingOps(), hasLength(2));
    expect(
      (await s.items(filter: const ItemFilter(unreadOnly: true))).length,
      1,
    );
  });

  test('digests, meta, settings, synced files round trip', () async {
    expect(await s.meta(), isNull);
    await s.putMeta(
      MetaFile(
        categories: [CategoryMeta(id: 'tech', label: 'Tech')],
        topics: [TopicMeta(id: 10, name: 'Rust')],
        sources: [],
      ),
    );
    expect((await s.meta())!.topics.single.name, 'Rust');
    await s.upsertDigests(
      DigestFile(
        period: 'day',
        periodStart: '2026-10-10',
        stats: {'n': 1},
        entries: [
          DigestRecord(
            scope: null,
            body: 'b',
            status: 'ok',
            itemIds: [1],
            provider: 'p',
            createdAt: '2026-10-10T00:00:00Z',
          ),
        ],
      ),
    );
    expect((await s.digests('day', '2026-10-10'))!.entries.single.body, 'b');
    expect(await s.digests('week', '2026-10-10'), isNull);
    await s.putSyncedFile('manifest.json', 'h1', 'manifest', imported: false);
    var f = (await s.syncedFile('manifest.json'))!;
    expect((f.sha256, f.kind, f.imported), ('h1', 'manifest', false));
    await s.putSyncedFile('manifest.json', 'h2', 'manifest');
    f = (await s.syncedFile('manifest.json'))!;
    expect((f.sha256, f.imported), ('h2', true));
    await s.setSetting('a', '1');
    await s.setSetting('a', '2');
    expect(await s.getSetting('a'), '2');
  });

  group('ItemFilter.ids, countItems, localCounts', () {
    test('ids limits rows and the count; empty ids matches nothing', () async {
      await seed();
      final one = await s.items(filter: const ItemFilter(ids: [2]));
      expect(one.map((i) => i.record.id), [2]);
      expect(await s.countItems(const ItemFilter(ids: [1, 3])), 2);
      expect(await s.items(filter: const ItemFilter(ids: [])), isEmpty);
      expect(await s.countItems(const ItemFilter(ids: [])), 0);
      expect(await s.countItems(const ItemFilter()), 3);
    });

    test('countItems follows the other filters and offset pages', () async {
      await seed();
      expect(await s.countItems(const ItemFilter(category: 'tech')), 2);
      expect(await s.countItems(const ItemFilter(topicId: 11)), 2);
      await s.setRead(1, true, 'phone', now: t(1));
      expect(await s.countItems(const ItemFilter(unreadOnly: true)), 2);
      final all = await s.items();
      final page = await s.items(limit: 2, offset: 1);
      expect(page.map((i) => i.record.id), all.skip(1).map((i) => i.record.id));
    });

    test('markAllRead respects ids when present', () async {
      await seed();
      final n = await s.markAllRead(const ItemFilter(ids: [1, 2]), 'phone');
      expect(n, 2);
      expect(await s.countItems(const ItemFilter(unreadOnly: true)), 1);
      expect(await s.markAllRead(const ItemFilter(ids: []), 'phone'), 0);
      expect(await s.markAllRead(const ItemFilter(), 'phone'), 1);
    });

    test('localCounts counts items and digests', () async {
      expect(await s.localCounts(), (items: 0, digests: 0));
      await seed();
      expect((await s.localCounts()).items, 3);
    });
  });
}

import 'package:flutter/foundation.dart';
import 'package:infodesk_mobile/app/controller.dart';
import 'package:infodesk_mobile/auth/google_account.dart';
import 'package:infodesk_mobile/auth/tailnet_service.dart';
import 'package:infodesk_mobile/data/local_store.dart';
import 'package:infodesk_mobile/settings/app_settings.dart';
import 'package:infodesk_mobile/storage/storage_factory.dart';
import 'package:infodesk_mobile/sync/engine.dart';
import 'package:infodesk_mobile/sync/format.dart';

ItemRecord rec(
  int id, {
  String title = 'title',
  String category = 'medcare',
  String? image,
  String? summary,
  List<int> topics = const [],
  String source = 'Google News',
  String excerpt = 'excerpt',
}) => ItemRecord(
  id: id,
  url: 'https://example.com/$id',
  title: title,
  excerpt: excerpt,
  aiSummary: summary,
  imageUrl: image,
  sourceId: 1,
  sourceName: source,
  category: category,
  score: null,
  publishedAt: null,
  fetchedAt: DateTime.now().toUtc().toIso8601String(),
  topicIds: topics,
);

/// In-memory stand-in for [LocalStore]. Records every write.
class FakeStore implements LocalStore {
  FakeStore({List<StoredItem>? items}) : all = items ?? [];

  final List<StoredItem> all;
  final settingsMap = <String, String>{};
  final pending = <Op>[];
  final digestFiles = <String, DigestFile>{};
  MetaFile? metaFile;
  int clearCalls = 0;

  static String _day() {
    final n = DateTime.now();
    return '${n.year.toString().padLeft(4, '0')}-'
        '${n.month.toString().padLeft(2, '0')}-${n.day.toString().padLeft(2, '0')}';
  }

  static StoredItem item(
    ItemRecord r, {
    bool read = false,
    bool starred = false,
    String? date,
  }) => StoredItem(
    record: r,
    date: date ?? _day(),
    read: read ? Stamp(value: true, at: 'a', device: 'd') : null,
    starred: starred ? Stamp(value: true, at: 'a', device: 'd') : null,
  );

  bool _match(StoredItem i, ItemFilter f) {
    if (f.periodStart != null && i.date.compareTo(f.periodStart!) < 0) {
      return false;
    }
    if (f.category != null && i.record.category != f.category) return false;
    if (f.topicId != null && !i.record.topicIds.contains(f.topicId)) {
      return false;
    }
    if (f.unreadOnly && i.isRead) return false;
    if (f.starredOnly && !i.isStarred) return false;
    if (f.ids != null && !f.ids!.contains(i.record.id)) return false;
    final q = f.search?.trim();
    if (q != null && q.isNotEmpty && !i.record.title.contains(q)) return false;
    return true;
  }

  @override
  Future<List<StoredItem>> items({
    ItemFilter filter = const ItemFilter(),
    int? limit,
    int? offset,
  }) async {
    final r = [
      for (final i in all)
        if (_match(i, filter)) i,
    ];
    final from = offset ?? 0;
    final tail = from >= r.length ? <StoredItem>[] : r.sublist(from);
    return limit == null ? tail : tail.take(limit).toList();
  }

  @override
  Future<int> countItems(ItemFilter filter) async =>
      all.where((i) => _match(i, filter)).length;

  @override
  Future<({int items, int digests})> localCounts() async =>
      (items: all.length, digests: digestFiles.length);

  @override
  Future<DigestFile?> digests(String period, String start) async =>
      digestFiles['$period|$start'];

  @override
  Future<MetaFile?> meta() async => metaFile;

  void _set(int id, Field f, bool v) {
    final idx = all.indexWhere((i) => i.record.id == id);
    final cur = all[idx];
    final s = Stamp(value: v, at: 'now', device: 'dev');
    all[idx] = StoredItem(
      record: cur.record,
      date: cur.date,
      read: f == Field.read ? s : cur.read,
      starred: f == Field.starred ? s : cur.starred,
    );
    pending.add(Op(itemId: id, field: f, value: v, at: 'now', device: 'dev'));
  }

  @override
  Future<bool> setRead(
    int id,
    bool value,
    String device, {
    DateTime? now,
  }) async {
    _set(id, Field.read, value);
    return true;
  }

  @override
  Future<bool> setStarred(
    int id,
    bool value,
    String device, {
    DateTime? now,
  }) async {
    _set(id, Field.starred, value);
    return true;
  }

  @override
  Future<int> markAllRead(
    ItemFilter filter,
    String device, {
    DateTime? now,
  }) async {
    var n = 0;
    for (final i in List.of(all)) {
      if (!i.isRead && _match(i, filter)) {
        _set(i.record.id, Field.read, true);
        n++;
      }
    }
    return n;
  }

  @override
  Future<String> deviceId() async => 'dev';

  @override
  Future<String?> getSetting(String key) async => settingsMap[key];

  @override
  Future<void> setSetting(String key, String value) async =>
      settingsMap[key] = value;

  @override
  Future<void> clearLocalCopy() async {
    clearCalls++;
    all.clear();
    digestFiles.clear();
  }

  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
}

class FakeEngine implements SyncEngine {
  int runs = 0;
  Future<void>? hold;
  Object? failWith;
  SyncReport report = SyncReport();

  @override
  Future<SyncReport> runOnce({DateTime? now}) async {
    runs++;
    await hold;
    final e = failWith;
    if (e != null) throw e;
    return report;
  }

  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
}

class FakeSettings implements AppSettings {
  Backend current = Backend.gdrive;
  NasConfig config = const NasConfig(url: '', user: '');
  final calls = <String>[];
  Object? failBackend;

  @override
  Future<Backend> backend() async {
    final e = failBackend;
    if (e != null) throw e;
    return current;
  }

  @override
  Future<bool> setBackend(Backend b) async {
    calls.add('setBackend:${b.name}');
    if (current == b) return false;
    current = b;
    return true;
  }

  @override
  Future<NasConfig> nas() async => config;

  @override
  Future<void> setNas({
    required String url,
    required String user,
    String? password,
  }) async {
    calls.add('setNas:$url:$user:${password ?? '-'}');
    final bad = url.trim().isEmpty ? null : validateNasUrl(url);
    if (bad != null) throw InvalidSetting(bad);
    config = NasConfig(
      url: url.trim(),
      user: user.trim(),
      password: password ?? config.password,
    );
  }

  @override
  Future<void> clearNas() async {
    calls.add('clearNas');
    config = const NasConfig(url: '', user: '');
  }

  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
}

class FakeGoogle implements GoogleAccount {
  String? email;
  Object? failWith;
  int signIns = 0;
  int signOuts = 0;

  @override
  String? get currentEmail => email;

  @override
  Future<void> restore() async {}

  @override
  Future<String?> signIn() async {
    signIns++;
    final e = failWith;
    if (e != null) throw e;
    return email = 'you@gmail.com';
  }

  @override
  Future<void> signOut() async {
    signOuts++;
    email = null;
  }

  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
}

class FakeTailnet implements TailnetService {
  @override
  final state = ValueNotifier<TailnetState>(const TailnetStopped());
  final calls = <String>[];

  /// When set, startOnLaunch does not finish until this completes.
  Future<void>? launchHold;
  final launches = <bool>[];

  @override
  Future<void> startOnLaunch({required bool nas}) async {
    launches.add(nas);
    await launchHold;
  }

  @override
  Future<void> start() async => calls.add('start');
  @override
  Future<void> stop() async => calls.add('stop');
  @override
  Future<void> login() async => calls.add('login');
  @override
  Future<void> logout() async => calls.add('logout');
  @override
  Future<void> forget() async => calls.add('forget');
  @override
  Future<bool> hasSavedState() async => false;

  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
}

class FakeStorage implements StorageFactory {
  String checkResult = nasCheckOk;
  int checks = 0;

  @override
  Future<String> checkNas() async {
    checks++;
    return checkResult;
  }

  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
}

class Harness {
  Harness({List<StoredItem>? items, bool onboarded = true}) {
    store = FakeStore(items: items);
    if (onboarded) store.settingsMap['onboarded'] = 'true';
    controller = AppController(
      store: store,
      engine: engine,
      settings: settings,
      storage: storage,
      google: google,
      tailnet: tailnet,
      openArticle: (u) async {
        opened.add(u);
        return true;
      },
      openExternal: (u) async {
        external.add(u);
        return true;
      },
      clock: () => clockNow,
      version: '1.0.0',
    );
  }

  late final FakeStore store;
  final engine = FakeEngine();
  final settings = FakeSettings();
  final storage = FakeStorage();
  final google = FakeGoogle()..email = 'you@gmail.com';
  final tailnet = FakeTailnet();
  final opened = <Uri>[];
  final external = <Uri>[];
  DateTime clockNow = DateTime(2026, 10, 15, 9, 41);
  late final AppController controller;
}

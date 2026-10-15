import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

// Port of crates/core/src/sync/format.rs. Keep the two in step; the shared
// fixtures in crates/core/tests/fixtures/sync/ test both.

const int schema = 1;

T _get<T>(Map<String, dynamic> j, String k) => j[k] as T;

List<T> _list<T>(Object? v) => (v as List).cast<T>().toList();

class FileEntry {
  FileEntry({
    required this.sha256,
    required this.size,
    required this.updatedAt,
  });
  final String sha256;
  final int size;
  final String updatedAt;
  factory FileEntry.fromJson(Map<String, dynamic> j) => FileEntry(
    sha256: _get(j, 'sha256'),
    size: _get(j, 'size'),
    updatedAt: _get(j, 'updated_at'),
  );
  Map<String, dynamic> toJson() => {
    'sha256': sha256,
    'size': size,
    'updated_at': updatedAt,
  };
}

class Manifest {
  Manifest({
    required this.schema,
    required this.dbId,
    required this.writtenAt,
    required this.writer,
    required this.files,
  });
  final int schema;
  final String dbId;
  final String writtenAt;
  final String writer;
  final Map<String, FileEntry> files;
  factory Manifest.fromJson(Map<String, dynamic> j) => Manifest(
    schema: _get(j, 'schema'),
    dbId: _get(j, 'db_id'),
    writtenAt: _get(j, 'written_at'),
    writer: _get(j, 'writer'),
    files: {
      for (final e in (j['files'] as Map<String, dynamic>).entries)
        e.key: FileEntry.fromJson(e.value as Map<String, dynamic>),
    },
  );
  Map<String, dynamic> toJson() => {
    'schema': schema,
    'db_id': dbId,
    'written_at': writtenAt,
    'writer': writer,
    'files': {for (final e in files.entries) e.key: e.value.toJson()},
  };
}

class CategoryMeta {
  CategoryMeta({required this.id, required this.label});
  final String id;
  final String label;
  factory CategoryMeta.fromJson(Map<String, dynamic> j) =>
      CategoryMeta(id: _get(j, 'id'), label: _get(j, 'label'));
  Map<String, dynamic> toJson() => {'id': id, 'label': label};
}

class TopicMeta {
  TopicMeta({required this.id, required this.name});
  final int id;
  final String name;
  factory TopicMeta.fromJson(Map<String, dynamic> j) =>
      TopicMeta(id: _get(j, 'id'), name: _get(j, 'name'));
  Map<String, dynamic> toJson() => {'id': id, 'name': name};
}

class SourceMeta {
  SourceMeta({required this.id, required this.name, required this.category});
  final int id;
  final String name;
  final String category;
  factory SourceMeta.fromJson(Map<String, dynamic> j) => SourceMeta(
    id: _get(j, 'id'),
    name: _get(j, 'name'),
    category: _get(j, 'category'),
  );
  Map<String, dynamic> toJson() => {
    'id': id,
    'name': name,
    'category': category,
  };
}

class MetaFile {
  MetaFile({
    required this.categories,
    required this.topics,
    required this.sources,
  });
  final List<CategoryMeta> categories;
  final List<TopicMeta> topics;
  final List<SourceMeta> sources;
  factory MetaFile.fromJson(Map<String, dynamic> j) => MetaFile(
    categories: [
      for (final c in j['categories'] as List)
        CategoryMeta.fromJson(c as Map<String, dynamic>),
    ],
    topics: [
      for (final c in j['topics'] as List)
        TopicMeta.fromJson(c as Map<String, dynamic>),
    ],
    sources: [
      for (final c in j['sources'] as List)
        SourceMeta.fromJson(c as Map<String, dynamic>),
    ],
  );
  Map<String, dynamic> toJson() => {
    'categories': [for (final c in categories) c.toJson()],
    'topics': [for (final c in topics) c.toJson()],
    'sources': [for (final c in sources) c.toJson()],
  };
}

class ItemRecord {
  ItemRecord({
    required this.id,
    required this.url,
    required this.title,
    required this.excerpt,
    required this.aiSummary,
    required this.imageUrl,
    required this.sourceId,
    required this.sourceName,
    required this.category,
    required this.score,
    required this.publishedAt,
    required this.fetchedAt,
    required this.topicIds,
  });
  final int id;
  final String url;
  final String title;
  final String excerpt;
  final String? aiSummary;
  final String? imageUrl;
  final int? sourceId;
  final String sourceName;
  final String category;
  final int? score;
  final String? publishedAt;
  final String fetchedAt;
  final List<int> topicIds;
  factory ItemRecord.fromJson(Map<String, dynamic> j) => ItemRecord(
    id: _get(j, 'id'),
    url: _get(j, 'url'),
    title: _get(j, 'title'),
    excerpt: _get(j, 'excerpt'),
    aiSummary: j['ai_summary'] as String?,
    imageUrl: j['image_url'] as String?,
    sourceId: j['source_id'] as int?,
    sourceName: _get(j, 'source_name'),
    category: _get(j, 'category'),
    score: j['score'] as int?,
    publishedAt: j['published_at'] as String?,
    fetchedAt: _get(j, 'fetched_at'),
    topicIds: _list<int>(j['topic_ids']),
  );
  Map<String, dynamic> toJson() => {
    'id': id,
    'url': url,
    'title': title,
    'excerpt': excerpt,
    'ai_summary': aiSummary,
    'image_url': imageUrl,
    'source_id': sourceId,
    'source_name': sourceName,
    'category': category,
    'score': score,
    'published_at': publishedAt,
    'fetched_at': fetchedAt,
    'topic_ids': topicIds,
  };
}

class DayFile {
  DayFile({required this.date, required this.items});
  final String date;
  final List<ItemRecord> items;
  factory DayFile.fromJson(Map<String, dynamic> j) => DayFile(
    date: _get(j, 'date'),
    items: [
      for (final i in j['items'] as List)
        ItemRecord.fromJson(i as Map<String, dynamic>),
    ],
  );
  Map<String, dynamic> toJson() => {
    'date': date,
    'items': [for (final i in items) i.toJson()],
  };
}

class DigestRecord {
  DigestRecord({
    required this.scope,
    required this.body,
    required this.status,
    required this.itemIds,
    required this.provider,
    required this.createdAt,
  });
  final Object? scope;
  final String body;
  final String status;
  final List<int> itemIds;
  final String provider;
  final String createdAt;
  factory DigestRecord.fromJson(Map<String, dynamic> j) => DigestRecord(
    scope: j['scope'],
    body: _get(j, 'body'),
    status: _get(j, 'status'),
    itemIds: _list<int>(j['item_ids']),
    provider: _get(j, 'provider'),
    createdAt: _get(j, 'created_at'),
  );
  Map<String, dynamic> toJson() => {
    'scope': scope,
    'body': body,
    'status': status,
    'item_ids': itemIds,
    'provider': provider,
    'created_at': createdAt,
  };
}

class DigestFile {
  DigestFile({
    required this.period,
    required this.periodStart,
    required this.stats,
    required this.entries,
  });
  final String period;
  final String periodStart;
  final Object? stats;
  final List<DigestRecord> entries;
  factory DigestFile.fromJson(Map<String, dynamic> j) => DigestFile(
    period: _get(j, 'period'),
    periodStart: _get(j, 'period_start'),
    stats: j['stats'],
    entries: [
      for (final e in j['entries'] as List)
        DigestRecord.fromJson(e as Map<String, dynamic>),
    ],
  );
  Map<String, dynamic> toJson() => {
    'period': period,
    'period_start': periodStart,
    'stats': stats,
    'entries': [for (final e in entries) e.toJson()],
  };
}

enum Field {
  read('read'),
  starred('starred');

  const Field(this.json);
  final String json;
  static Field parse(String s) => Field.values.firstWhere(
    (f) => f.json == s,
    orElse: () => throw FormatException('unknown field: $s'),
  );
}

class Op {
  Op({
    required this.itemId,
    required this.field,
    required this.value,
    required this.at,
    required this.device,
  });
  final int itemId;
  final Field field;
  final bool value;
  final String at;
  final String device;
  factory Op.fromJson(Map<String, dynamic> j) => Op(
    itemId: _get(j, 'item_id'),
    field: Field.parse(_get(j, 'field')),
    value: _get(j, 'value'),
    at: _get(j, 'at'),
    device: _get(j, 'device'),
  );
  Op copyWith({String? at}) => Op(
    itemId: itemId,
    field: field,
    value: value,
    at: at ?? this.at,
    device: device,
  );
  Map<String, dynamic> toJson() => {
    'item_id': itemId,
    'field': field.json,
    'value': value,
    'at': at,
    'device': device,
  };
}

class OpsFile {
  OpsFile({required this.device, required this.ops});
  final String device;
  final List<Op> ops;
  factory OpsFile.fromJson(Map<String, dynamic> j) => OpsFile(
    device: _get(j, 'device'),
    ops: [
      for (final o in j['ops'] as List) Op.fromJson(o as Map<String, dynamic>),
    ],
  );
  Map<String, dynamic> toJson() => {
    'device': device,
    'ops': [for (final o in ops) o.toJson()],
  };
}

class Stamp {
  Stamp({required this.value, required this.at, required this.device});
  final bool value;
  final String at;
  final String device;
  factory Stamp.fromJson(Map<String, dynamic> j) => Stamp(
    value: _get(j, 'value'),
    at: _get(j, 'at'),
    device: _get(j, 'device'),
  );
  Map<String, dynamic> toJson() => {'value': value, 'at': at, 'device': device};
}

class ItemState {
  ItemState({this.read, this.starred});
  Stamp? read;
  Stamp? starred;
  factory ItemState.fromJson(Map<String, dynamic> j) => ItemState(
    read: j['read'] == null
        ? null
        : Stamp.fromJson(j['read'] as Map<String, dynamic>),
    starred: j['starred'] == null
        ? null
        : Stamp.fromJson(j['starred'] as Map<String, dynamic>),
  );
  Map<String, dynamic> toJson() => {
    'read': read?.toJson(),
    'starred': starred?.toJson(),
  };
}

class StateSnapshot {
  StateSnapshot({required this.covers, required this.state});
  final List<String> covers;
  final Map<int, ItemState> state;

  /// State keys arrive as JSON strings and become ints here.
  factory StateSnapshot.fromJson(Map<String, dynamic> j) => StateSnapshot(
    covers: _list<String>(j['covers']),
    state: {
      for (final e in (j['state'] as Map<String, dynamic>).entries)
        int.parse(e.key): ItemState.fromJson(e.value as Map<String, dynamic>),
    },
  );
  Map<String, dynamic> toJson() => {
    'covers': covers,
    'state': {
      for (final k in state.keys.toList()..sort()) '$k': state[k]!.toJson(),
    },
  };
}

enum FileKindType { manifest, meta, snapshot, day, digest, ops }

class FileKind {
  const FileKind(this.kind, {this.date, this.period, this.device});
  final FileKindType kind;
  final String? date;
  final String? period;
  final String? device;
}

const _periods = ['day', 'week', 'month'];

String _p(int n, int w) => n.toString().padLeft(w, '0');

bool _leap(int y) => (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;

int _daysIn(int y, int m) =>
    const [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][m - 1] +
    (m == 2 && _leap(y) ? 1 : 0);

bool _validDate(int y, int m, int d) =>
    m >= 1 && m <= 12 && d >= 1 && d <= _daysIn(y, m);

final _dateRe = RegExp(r'^(\d{4})-(\d{2})-(\d{2})$');

/// The date string when it is a real calendar date in canonical form.
String? _canonicalDate(String s) {
  final m = _dateRe.firstMatch(s);
  if (m == null) return null;
  final y = int.parse(m[1]!), mo = int.parse(m[2]!), d = int.parse(m[3]!);
  return _validDate(y, mo, d) ? s : null;
}

String dayFileName(String date) => 'day-$date.json.gz';

String digestFileName(String period, String start) =>
    'digest-$period-$start.json.gz';

/// Throws [RangeError] unless `seq` is within 1 to 9999 (4 digits).
String opsFileName(String device, DateTime at, int seq) {
  RangeError.checkValueInInterval(seq, 1, 9999, 'seq');
  final u = at.toUtc();
  return 'ops-$device-${_p(u.year, 4)}${_p(u.month, 2)}${_p(u.day, 2)}'
      'T${_p(u.hour, 2)}${_p(u.minute, 2)}${_p(u.second, 2)}'
      '${_p(u.millisecond, 3)}Z-${_p(seq, 4)}.json';
}

final _stampRe = RegExp(
  r'^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})(\d{3})Z$',
);

bool _isOpsStamp(String s) {
  final m = _stampRe.firstMatch(s);
  if (m == null) return false;
  final y = int.parse(m[1]!), mo = int.parse(m[2]!), d = int.parse(m[3]!);
  return _validDate(y, mo, d) &&
      int.parse(m[4]!) < 24 &&
      int.parse(m[5]!) < 60 &&
      int.parse(m[6]!) <= 60;
}

final _digitsRe = RegExp(r'^\d{4}$');

FileKind? parseFileName(String name) {
  switch (name) {
    case 'manifest.json':
      return const FileKind(FileKindType.manifest);
    case 'meta.json.gz':
      return const FileKind(FileKindType.meta);
    case 'state-snapshot.json.gz':
      return const FileKind(FileKindType.snapshot);
  }
  const gz = '.json.gz';
  if (name.startsWith('day-') &&
      name.endsWith(gz) &&
      name.length >= 4 + gz.length) {
    final date = _canonicalDate(name.substring(4, name.length - gz.length));
    return date == null ? null : FileKind(FileKindType.day, date: date);
  }
  if (name.startsWith('digest-') &&
      name.endsWith(gz) &&
      name.length >= 7 + gz.length) {
    final rest = name.substring(7, name.length - gz.length);
    final i = rest.indexOf('-');
    if (i < 0) return null;
    final period = rest.substring(0, i);
    if (!_periods.contains(period)) return null;
    final date = _canonicalDate(rest.substring(i + 1));
    return date == null
        ? null
        : FileKind(FileKindType.digest, period: period, date: date);
  }
  if (name.startsWith('ops-') && name.endsWith('.json') && name.length >= 9) {
    // <device>-<YYYYMMDDTHHMMSSmmmZ>-<NNNN>
    final rest = name.substring(4, name.length - 5);
    final a = rest.lastIndexOf('-');
    if (a < 0) return null;
    final seq = rest.substring(a + 1);
    if (!_digitsRe.hasMatch(seq)) return null;
    final head = rest.substring(0, a);
    final b = head.lastIndexOf('-');
    if (b < 0) return null;
    final device = head.substring(0, b);
    if (device.isEmpty || !_isOpsStamp(head.substring(b + 1))) return null;
    return FileKind(FileKindType.ops, device: device);
  }
  return null;
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` in UTC.
String opTime(DateTime at) {
  final u = at.toUtc();
  return '${_p(u.year, 4)}-${_p(u.month, 2)}-${_p(u.day, 2)}'
      'T${_p(u.hour, 2)}:${_p(u.minute, 2)}:${_p(u.second, 2)}'
      '.${_p(u.millisecond, 3)}Z';
}

final _rfc3339 = RegExp(
  r'^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?'
  r'(?:[Zz]|([+\-\u2212])(\d{2}):(\d{2}))$',
);

int _daysFromCivil(int y, int m, int d) {
  final yy = m <= 2 ? y - 1 : y;
  final era = (yy >= 0 ? yy : yy - 399) ~/ 400;
  final yoe = yy - era * 400;
  final doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) ~/ 5 + d - 1;
  final doe = yoe * 365 + yoe ~/ 4 - yoe ~/ 100 + doy;
  return era * 146097 + doe - 719468;
}

/// Parses RFC3339 text to UTC op_time text, or null. Sub-millisecond digits
/// are truncated, like the Mac.
String? _rfc3339ToOpTime(String s) {
  final m = _rfc3339.firstMatch(s);
  if (m == null) return null;
  final y = int.parse(m[1]!), mo = int.parse(m[2]!), d = int.parse(m[3]!);
  final h = int.parse(m[4]!), mi = int.parse(m[5]!), se = int.parse(m[6]!);
  // Deliberate divergence from chrono: a leap second (:60) in an op's `at` is
  // rejected, since it has no millisecond epoch. The op is dropped. File-name
  // stamps still accept :60 on both sides.
  if (!_validDate(y, mo, d) || h > 23 || mi > 59 || se > 59) return null;
  var offMin = 0;
  if (m[8] != null) {
    final oh = int.parse(m[9]!), om = int.parse(m[10]!);
    if (oh > 23 || om > 59) return null;
    offMin = (oh * 60 + om) * (m[8] == '+' ? 1 : -1);
  }
  final millis = int.parse(('${m[7] ?? ''}000').substring(0, 3));
  final epochMs =
      (((_daysFromCivil(y, mo, d) * 24 + h) * 60 + mi - offMin) * 60 + se) *
          1000 +
      millis;
  return opTime(DateTime.fromMillisecondsSinceEpoch(epochMs, isUtc: true));
}

/// Normalizes an op from another client: `at` becomes UTC `opTime` text.
/// Returns null for an unparsable `at` or an empty `device`.
Op? normalizeOp(Op op) {
  if (op.device.isEmpty) return null;
  final at = _rfc3339ToOpTime(op.at);
  return at == null ? null : op.copyWith(at: at);
}

/// True when `a` beats `b`: newer `at`, then larger `device`, then value true.
/// Callers must pass normalized ops.
bool wins(Stamp a, Stamp b) {
  var c = a.at.compareTo(b.at);
  if (c != 0) return c > 0;
  c = a.device.compareTo(b.device);
  if (c != 0) return c > 0;
  return a.value && !b.value;
}

/// Callers must pass normalized ops.
void applyOp(Map<int, ItemState> state, Op op) {
  final entry = state.putIfAbsent(op.itemId, ItemState.new);
  final cur = op.field == Field.read ? entry.read : entry.starred;
  final stamp = Stamp(value: op.value, at: op.at, device: op.device);
  if (cur == null || wins(stamp, cur)) {
    if (op.field == Field.read) {
      entry.read = stamp;
    } else {
      entry.starred = stamp;
    }
  }
}

Uint8List gzipJson(Object? json) =>
    Uint8List.fromList(gzip.encode(utf8.encode(jsonEncode(json))));

dynamic gunzipJson(List<int> bytes) =>
    jsonDecode(utf8.decode(gzip.decode(bytes)));

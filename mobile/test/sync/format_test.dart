import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/sync/format.dart';

dynamic fixture(String name) => jsonDecode(
  File('../crates/core/tests/fixtures/sync/$name').readAsStringSync(),
);

void main() {
  test('shared file name fixtures parse the same as the Mac', () {
    for (final c in fixture('file_names.json') as List) {
      final name = c['name'] as String;
      final got = parseFileName(name);
      final kind = c['kind'] as String?;
      if (kind == null) {
        expect(got, isNull, reason: name);
        continue;
      }
      expect(got, isNotNull, reason: name);
      expect(got!.kind.name, kind, reason: name);
      expect(got.date, c['date'], reason: name);
      expect(got.period, c['period'], reason: name);
      expect(got.device, c['device'], reason: name);
    }
  });

  test('file name builders', () {
    expect(dayFileName('2026-10-13'), 'day-2026-10-13.json.gz');
    expect(
      opsFileName('dev-a', DateTime.utc(2026, 10, 13, 1, 2, 3, 456), 7),
      'ops-dev-a-20261013T010203456Z-0007.json',
    );
    expect(
      parseFileName('ops-dev-a-20261013T010203456Z-0007.json')!.device,
      'dev-a',
    );
  });

  test('opTime has millis and Z', () {
    expect(
      opTime(DateTime.utc(2026, 10, 13, 1, 2, 3, 456, 700)),
      '2026-10-13T01:02:03.456Z',
    );
  });

  test('normalizeOp converts foreign times and rejects garbage', () {
    Op op(String at, [String device = 'p']) =>
        Op(itemId: 1, field: Field.read, value: true, at: at, device: device);
    expect(
      normalizeOp(op('2026-10-13T09:00:03+09:00'))!.at,
      '2026-10-13T00:00:03.000Z',
    );
    expect(
      normalizeOp(op('2026-10-13T00:00:01.400123Z'))!.at,
      '2026-10-13T00:00:01.400Z',
    );
    expect(
      normalizeOp(op('2026-10-13T00:00:02Z'))!.at,
      '2026-10-13T00:00:02.000Z',
    );
    expect(
      normalizeOp(op('2026-10-13T00:30:00-05:30'))!.at,
      '2026-10-13T06:00:00.000Z',
    );
    expect(normalizeOp(op('9999')), isNull);
    expect(normalizeOp(op('')), isNull);
    expect(normalizeOp(op('2026-10-13T00:00:00')), isNull);
    expect(normalizeOp(op('2026-10-13T00:00:00Z', '')), isNull);
  });

  test('shared ops merge fixture gives the expected state', () {
    final v = fixture('ops_merge.json');
    final ops = [for (final o in v['ops'] as List) Op.fromJson(o)];
    final want = {
      for (final e in (v['expected_state'] as Map).entries)
        int.parse(e.key as String): ItemState.fromJson(e.value),
    };
    final forward = <int, ItemState>{};
    for (final o in ops) {
      final n = normalizeOp(o);
      if (n != null) applyOp(forward, n);
    }
    final backward = <int, ItemState>{};
    for (final o in ops.reversed) {
      final n = normalizeOp(o);
      if (n != null) applyOp(backward, n);
    }
    expect(jsonEncode(_enc(forward)), jsonEncode(_enc(want)));
    expect(jsonEncode(_enc(backward)), jsonEncode(_enc(want)));
  });

  test('wins breaks ties on at, device, then value', () {
    Stamp s(bool v, String at, String d) => Stamp(value: v, at: at, device: d);
    expect(
      wins(
        s(false, '2026-10-13T00:00:02.000Z', 'a'),
        s(true, '2026-10-13T00:00:01.000Z', 'z'),
      ),
      isTrue,
    );
    expect(
      wins(
        s(false, '2026-10-13T00:00:01.000Z', 'b'),
        s(true, '2026-10-13T00:00:01.000Z', 'a'),
      ),
      isTrue,
    );
    expect(
      wins(
        s(true, '2026-10-13T00:00:01.000Z', 'a'),
        s(false, '2026-10-13T00:00:01.000Z', 'a'),
      ),
      isTrue,
    );
    expect(
      wins(
        s(false, '2026-10-13T00:00:01.000Z', 'a'),
        s(true, '2026-10-13T00:00:01.000Z', 'a'),
      ),
      isFalse,
    );
  });

  test('snapshot fixture converts string keys to ints and back', () {
    final raw = fixture('snapshot.json');
    final snap = StateSnapshot.fromJson(raw);
    expect(snap.covers.length, 2);
    expect(snap.state.keys.toList(), [10, 42]);
    expect(snap.state[10]!.read!.value, isTrue);
    expect(snap.state[42]!.read, isNull);
    expect(jsonEncode(snap.toJson()), jsonEncode(raw));
  });

  test('gzip round trip and reading a standard gzip stream', () {
    final f = OpsFile(
      device: 'd',
      ops: [
        Op(
          itemId: 1,
          field: Field.starred,
          value: true,
          at: '2026-10-13T00:00:00.000Z',
          device: 'd',
        ),
      ],
    );
    final back = OpsFile.fromJson(gunzipJson(gzipJson(f.toJson())));
    expect(jsonEncode(back.toJson()), jsonEncode(f.toJson()));
    expect(gunzipJson(gzip.encode(utf8.encode('{"a":[1,"日本"]}'))), {
      'a': [1, '日本'],
    });
  });

  test('record types parse the Mac JSON', () {
    final m = Manifest.fromJson({
      'schema': 1,
      'db_id': 'x',
      'written_at': 't',
      'writer': 'mac',
      'files': {
        'a': {'sha256': 'h', 'size': 3, 'updated_at': 'u'},
      },
    });
    expect(m.files['a']!.size, 3);
    final day = DayFile.fromJson({
      'date': '2026-10-13',
      'items': [
        {
          'id': 1,
          'url': 'u',
          'title': 't',
          'excerpt': 'e',
          'ai_summary': null,
          'image_url': null,
          'source_id': null,
          'source_name': 's',
          'category': 'c',
          'score': null,
          'published_at': null,
          'fetched_at': 'f',
          'topic_ids': [1, 2],
        },
      ],
    });
    expect(day.items.single.topicIds, [1, 2]);
    final digest = DigestFile.fromJson({
      'period': 'week',
      'period_start': '2026-10-05',
      'stats': null,
      'entries': [
        {
          'scope': {'k': 1},
          'body': 'b',
          'status': 'done',
          'item_ids': [1],
          'provider': 'p',
          'created_at': 'c',
        },
      ],
    });
    expect(digest.entries.single.itemIds, [1]);
    final meta = MetaFile.fromJson({
      'categories': [
        {'id': 'c', 'label': 'L'},
      ],
      'topics': [
        {'id': 1, 'name': 'n'},
      ],
      'sources': [
        {'id': 2, 'name': 's', 'category': 'c'},
      ],
    });
    expect(meta.sources.single.id, 2);
  });
}

Object _enc(Map<int, ItemState> m) => {
  for (final k in (m.keys.toList()..sort())) '$k': m[k]!.toJson(),
};

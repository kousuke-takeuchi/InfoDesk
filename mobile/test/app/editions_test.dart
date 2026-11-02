import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/app/editions.dart';
import 'package:infodesk_mobile/sync/format.dart';

DigestRecord entry(Object made, {String status = 'done'}) => DigestRecord(
  scope: {'category': 'medcare'},
  body: 'b',
  status: status,
  itemIds: const [],
  provider: 'p',
  createdAt: made is DateTime ? made.toUtc().toIso8601String() : made as String,
);

final now = DateTime(2026, 11, 2, 18);

({String title, String range}) heading(String start, List<DigestRecord> e) =>
    editionHeading(editionOf(start, e)!, now: now);

void main() {
  test('made during the day is that day\'s evening edition', () {
    final e = editionOf('2026-11-02', [entry(DateTime(2026, 11, 2, 17, 3))])!;
    expect(e.kind, EditionKind.evening);
    expect(e.issue, DateTime(2026, 11, 2));
    expect(e.cutoff, DateTime(2026, 11, 2, 17, 3));
    expect(heading('2026-11-02', [entry(DateTime(2026, 11, 2, 17, 3))]), (
      title: '11月2日 夕刊',
      range: '11月2日 17時3分までの記事',
    ));
    expect(
      heading('2026-11-02', [entry(DateTime(2026, 11, 2, 17))]).range,
      '11月2日 17時までの記事',
    );
    expect(
      heading('2026-11-02', [entry(DateTime(2026, 11, 2, 23, 59))]).range,
      '11月2日 23時59分までの記事',
    );
  });

  test('made after the day ends is the next day\'s morning edition', () {
    expect(heading('2026-11-01', [entry(DateTime(2026, 11, 2, 7, 5))]), (
      title: '11月2日 朝刊',
      range: '11月1日の記事',
    ));
    // Exactly midnight already belongs to the next day.
    final e = editionOf('2026-11-01', [entry(DateTime(2026, 11, 2))])!;
    expect(e.kind, EditionKind.morning);
    expect(e.cutoff, isNull);
  });

  test('the newest finished entry decides; others are ignored', () {
    final mixed = [
      entry(DateTime(2026, 11, 2, 17)),
      entry(DateTime(2026, 11, 3, 7)),
    ];
    expect(editionOf('2026-11-02', mixed)!.kind, EditionKind.morning);
    final pendingLater = [
      entry(DateTime(2026, 11, 2, 17)),
      entry(DateTime(2026, 11, 3, 7), status: 'pending'),
      entry('garbage'),
    ];
    expect(editionOf('2026-11-02', pendingLater)!.kind, EditionKind.evening);
  });

  test('no finished or readable entry means no edition', () {
    expect(editionOf('2026-11-02', const []), isNull);
    expect(
      editionOf('2026-11-02', [entry(DateTime(2026, 11, 2, 17), status: 'failed')]),
      isNull,
    );
    expect(editionOf('2026-11-02', [entry('garbage')]), isNull);
  });

  test('a time before the day gives no cutoff', () {
    expect(heading('2026-11-02', [entry(DateTime(2026, 11, 1, 23))]), (
      title: '11月2日 夕刊',
      range: '11月2日の記事',
    ));
  });

  test('month and year boundaries', () {
    expect(heading('2026-10-31', [entry(DateTime(2026, 11, 1, 7))]), (
      title: '11月1日 朝刊',
      range: '10月31日の記事',
    ));
    final e = editionOf('2026-12-31', [entry(DateTime(2027, 1, 1, 7))])!;
    expect(editionHeading(e, now: DateTime(2027, 1, 5)), (
      title: '1月1日 朝刊',
      range: '2026年12月31日の記事',
    ));
  });
}

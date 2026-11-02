import '../sync/format.dart';

enum EditionKind { morning, evening }

/// A stored day digest seen as a newspaper edition.
class Edition {
  const Edition({
    required this.kind,
    required this.issue,
    required this.articles,
    this.cutoff,
  });

  final EditionKind kind;

  /// The day the edition is named after: D+1 for a morning edition, D for an
  /// evening one (local midnight).
  final DateTime issue;

  /// The day whose articles it covers (D, local midnight).
  final DateTime articles;

  /// When an evening edition was made (local). Null for a morning edition,
  /// or when the time is before the day itself.
  final DateTime? cutoff;
}

/// The edition of the day digest for [start] (YYYY-MM-DD). The newest
/// `created_at` among finished entries decides: during D it is D's evening
/// edition, from the end of D on it is D+1's morning edition (D's final
/// version). Null when no entry is finished or no time can be read.
Edition? editionOf(String start, List<DigestRecord> entries) {
  final day = DateTime.parse(start);
  DateTime? newest;
  for (final e in entries) {
    if (e.status != 'done') continue;
    final t = DateTime.tryParse(e.createdAt)?.toLocal();
    if (t != null && (newest == null || t.isAfter(newest))) newest = t;
  }
  if (newest == null) return null;
  final next = DateTime(day.year, day.month, day.day + 1);
  if (!newest.isBefore(next)) {
    return Edition(kind: EditionKind.morning, issue: next, articles: day);
  }
  return Edition(
    kind: EditionKind.evening,
    issue: day,
    articles: day,
    cutoff: newest.isBefore(day) ? null : newest,
  );
}

/// 「11月2日 朝刊」/「11月1日の記事」, 「11月2日 夕刊」/「11月2日 17時までの記事」.
/// The year is shown only when it differs from [now]'s.
({String title, String range}) editionHeading(
  Edition e, {
  required DateTime now,
}) {
  String md(DateTime d) =>
      '${d.year == now.year ? '' : '${d.year}年'}${d.month}月${d.day}日';
  if (e.kind == EditionKind.morning) {
    return (title: '${md(e.issue)} 朝刊', range: '${md(e.articles)}の記事');
  }
  final c = e.cutoff;
  final until = c == null
      ? 'の記事'
      : ' ${c.hour}時${c.minute == 0 ? '' : '${c.minute}分'}までの記事';
  return (title: '${md(e.issue)} 夕刊', range: '${md(e.articles)}$until');
}

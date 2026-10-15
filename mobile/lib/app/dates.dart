const _weekdays = ['月', '火', '水', '木', '金', '土', '日'];

String two(int n) => n.toString().padLeft(2, '0');

String dateString(DateTime d) =>
    '${d.year.toString().padLeft(4, '0')}-'
    '${two(d.month)}-${two(d.day)}';

DateTime dayOnly(DateTime d) => DateTime(d.year, d.month, d.day);

enum DigestPeriod {
  day('day', '今日'),
  week('week', '週'),
  month('month', '月');

  const DigestPeriod(this.id, this.label);
  final String id;
  final String label;
}

/// Local midnight at the start of the period. Weeks start on Monday.
DateTime periodStart(DigestPeriod p, DateTime now) {
  final d = dayOnly(now);
  switch (p) {
    case DigestPeriod.day:
      return d;
    case DigestPeriod.week:
      return DateTime(d.year, d.month, d.day - (d.weekday - 1));
    case DigestPeriod.month:
      return DateTime(d.year, d.month, 1);
  }
}

String _dateJa(DateTime d, {required bool year}) =>
    '${year ? '${d.year}年' : ''}${d.month}月${d.day}日(${_weekdays[d.weekday - 1]})';

({String title, String range}) periodHeading(DigestPeriod p, DateTime now) {
  final start = periodStart(p, now);
  switch (p) {
    case DigestPeriod.day:
      return (title: '今日のダイジェスト', range: _dateJa(start, year: false));
    case DigestPeriod.week:
      final last = DateTime(start.year, start.month, start.day + 6);
      return (
        title: '今週のダイジェスト',
        range: '${_dateJa(start, year: false)} 〜 ${_dateJa(last, year: false)}',
      );
    case DigestPeriod.month:
      return (title: '今月のダイジェスト', range: '${start.year}年${start.month}月');
  }
}

/// Same wording as the Mac's `relativeTime`.
String relativeTime(DateTime t, DateTime now) {
  final diffMin = now.difference(t).inMinutes;
  if (diffMin < 1) return 'たった今';
  if (diffMin < 60) return '$diffMin分前';
  final hours = diffMin ~/ 60;
  final days = dayOnly(now).difference(dayOnly(t)).inDays;
  if (hours < 24 && days == 0) return '$hours時間前';
  if (days <= 0) return '${hours < 1 ? 1 : hours}時間前';
  if (days == 1) return '昨日';
  if (days < 7) return '$days日前';
  return '${t.month}月${t.day}日';
}

/// `9:42`, as in the designs.
String clock(DateTime t) => '${t.hour}:${two(t.minute)}';

/// `9:42` for today, `10月14日 9:42` for any other day.
String clockOrDate(DateTime t, DateTime now) =>
    dayOnly(t) == dayOnly(now) ? clock(t) : '${t.month}月${t.day}日 ${clock(t)}';

/// The digest body parser of `app/src/lib/digest.ts`, same rules.
class ParsedDigest {
  ParsedDigest(this.headline, this.bullets, this.paragraphs);
  final String? headline;
  final List<String> bullets;
  final List<String> paragraphs;
}

final _bullet = RegExp(r'^\s*(?:[-*]\s+|[・•]\s*|\d+[.)]\s+)(.*)$');
final _rule = RegExp(r'^(?:[-*_]\s*){3,}$');
final _markerOnly = RegExp(r'^[-*・•]$');
final _heading = RegExp(r'^#{1,6}\s+(.*)$');
final _headline = RegExp(
  r'^(?:#{1,6}\s+)?(\*\*|__)?見出し\s*(?:\1)?\s*[:：]\s*(?:\1)?\s*(.*)$',
);
final _bold = RegExp(r'(\*\*|__)(.+?)\1');

String stripBold(String text) =>
    text.replaceAllMapped(_bold, (m) => m[2]!).trim();

ParsedDigest parseDigestBody(String body) {
  final bullets = <String>[];
  final paragraphs = <String>[];
  String? headline;
  var first = true;
  for (final raw in body.split(RegExp(r'\r?\n'))) {
    final line = raw.trim();
    if (line.isEmpty || _rule.hasMatch(line) || _markerOnly.hasMatch(line)) {
      continue;
    }
    if (first) {
      first = false;
      final hm = _headline.firstMatch(line);
      if (hm != null) {
        var text = stripBold(hm[2]!);
        final marker = hm[1];
        if (marker != null && text.endsWith(marker)) {
          text = text.substring(0, text.length - marker.length).trim();
        }
        headline = text.isEmpty ? null : text;
        continue;
      }
    }
    final m = _bullet.firstMatch(line);
    if (m != null) {
      final text = stripBold(m[1]!);
      if (text.isNotEmpty) bullets.add(text);
      continue;
    }
    final h = _heading.firstMatch(line);
    final text = stripBold(h != null ? h[1]! : line);
    if (text.isNotEmpty) paragraphs.add(text);
  }
  return ParsedDigest(headline, bullets, paragraphs);
}

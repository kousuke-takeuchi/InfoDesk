import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/app/digest_text.dart';

void main() {
  test('plain headline, bullets and paragraphs', () {
    final p = parseDigestBody('見出し: 要約の題\n- one\n2. two\n本文です');
    expect(p.headline, '要約の題');
    expect(p.bullets, ['one', 'two']);
    expect(p.paragraphs, ['本文です']);
  });

  test('bold and heading forms of the headline', () {
    expect(parseDigestBody('**見出し**: 題').headline, '題');
    expect(parseDigestBody('**見出し:** 題').headline, '題');
    expect(parseDigestBody('**見出し: 題**').headline, '題');
    expect(parseDigestBody('## 見出し：題').headline, '題');
  });

  test('the headline counts only on the first non-empty line', () {
    final p = parseDigestBody('\n---\n- a\n見出し: late');
    expect(p.headline, isNull);
    expect(p.bullets, ['a']);
    expect(p.paragraphs, ['見出し: late']);
  });

  test('stripBold and lone markers', () {
    final p = parseDigestBody('- **強い** 点\n-\n## 小見出し');
    expect(p.bullets, ['強い 点']);
    expect(p.paragraphs, ['小見出し']);
  });
}

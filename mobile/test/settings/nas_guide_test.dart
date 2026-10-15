import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/settings/nas_guide.dart';

void main() {
  test('the NAS guide text matches the Mac dialog', () {
    expect(nasGuideTitle, 'NAS の準備');
    expect(nasGuideIntro, 'Synology NAS を InfoDesk の保存先にします。最初の1回だけ行います。');
    expect(nasGuideSteps.map((s) => s.title), [
      'DSM で WebDAV Server を入れる',
      '共有フォルダを作る',
      'InfoDesk 用のユーザーを作る',
      'NAS に Tailscale を入れる',
      'InfoDesk でつなぐ',
    ]);
    expect(nasGuideSteps.map((s) => s.body), [
      'パッケージセンターで「WebDAV Server」を入れ、HTTP (ポート 5005) を有効にします。',
      'コントロールパネル → 共有フォルダで「infodesk」を作ります。',
      'コントロールパネル → ユーザーで新しいユーザーを作り、infodesk だけ「読み取り/書き込み」、ほかの共有フォルダは「アクセス不可」にします。',
      'パッケージセンターで「Tailscale」を入れてログインし、NAS の名前 (例: takeuchi-nas) を控えます。',
      '保存先に「NAS (WebDAV)」を選び、Tailscale にログイン (NAS と同じアカウント)。URL に http://NAS の名前:5005/infodesk/、ユーザー名とパスワードを入れて「確かめる」を押します。',
    ]);
    expect(
      nasGuideTip,
      'Mac や iPhone に Tailscale のアプリは要りません。InfoDesk の中で Tailscale を使うので、ルーターのポートを開けず、ほかのアプリの通信にも影響しません。',
    );
    expect(
      nasGuideUrl,
      'https://github.com/kousuke-takeuchi/InfoDesk/blob/master/docs/13-nas-setup.ja.md',
    );
    expect(nasGuideLinkLabel, '詳しい手順 (docs/13-nas-setup)');
  });

  test('every text is present in the Mac dialog source', () {
    final f = File('../app/src/views/settings/NasGuideDialog.tsx');
    if (!f.existsSync()) return;
    final src = f.readAsStringSync();
    for (final t in [
      nasGuideTitle,
      nasGuideIntro,
      nasGuideTip,
      nasGuideLinkLabel,
      nasGuideUrl,
      for (final s in nasGuideSteps) ...[s.title, s.body],
    ]) {
      expect(src, contains(t));
    }
  });
}

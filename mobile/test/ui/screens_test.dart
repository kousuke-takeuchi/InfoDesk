import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/app/app.dart';
import 'package:infodesk_mobile/app/dates.dart';
import 'package:infodesk_mobile/auth/tailnet_service.dart';
import 'package:infodesk_mobile/data/local_store.dart';
import 'package:infodesk_mobile/settings/app_settings.dart';
import 'package:infodesk_mobile/storage/drive_store.dart';
import 'package:infodesk_mobile/sync/format.dart';

import 'fakes.dart';

Future<Harness> pumpApp(
  WidgetTester tester, {
  List<StoredItem>? items,
  bool onboarded = true,
  void Function(Harness)? setup,
}) async {
  // An iPhone-sized screen.
  tester.view.physicalSize = const Size(1170, 2532);
  tester.view.devicePixelRatio = 3;
  addTearDown(tester.view.reset);
  final h = Harness(items: items, onboarded: onboarded);
  setup?.call(h);
  await h.controller.init();
  await tester.pumpWidget(InfoDeskApp(controller: h.controller));
  await tester.pumpAndSettle();
  return h;
}

Future<void> tab(WidgetTester tester, String key) async {
  await tester.tap(find.byKey(Key('tab-$key')));
  await tester.pumpAndSettle();
}

DigestFile digestFile({String body = ''}) => DigestFile(
  period: 'day',
  periodStart: dateString(DateTime.now()),
  stats: jsonDecode(
    jsonEncode({
      'period': 'day',
      'period_start': dateString(DateTime.now()),
      'total': 156,
      'categories': [
        {'category': 'arxiv', 'count': 42, 'daily': <int>[]},
        {'category': 'medcare', 'count': 14, 'daily': <int>[]},
      ],
      'topics': [
        {'topic_id': 1, 'name': '認知症', 'count': 12, 'prev_count': 7},
        {'topic_id': 2, 'name': '介護', 'count': 9, 'prev_count': 11},
        {'topic_id': 3, 'name': 'ローカルLLM', 'count': 5, 'prev_count': 5},
      ],
    }),
  ),
  entries: [
    DigestRecord(
      scope: {'category': 'medcare'},
      body: body,
      status: 'done',
      itemIds: [1],
      provider: 'claude',
      createdAt: '2026-10-15T00:00:00.000Z',
    ),
  ],
);

List<StoredItem> sample() => [
  FakeStore.item(rec(1, title: '認知症の新薬', summary: '新薬の要約です', topics: [1])),
  FakeStore.item(rec(2, title: '介護報酬の改定'), starred: true),
  FakeStore.item(rec(3, title: '既読の記事'), read: true),
];

void main() {
  group('digest (M-1)', () {
    testWidgets('renders overview, topics, headline and numbered points', (
      tester,
    ) async {
      final h = await pumpApp(
        tester,
        items: [
          FakeStore.item(
            rec(1, title: '認知症の新薬', image: 'https://example.com/a.png'),
          ),
        ],
        setup: (h) {
          h.store.metaFile = MetaFile(
            categories: [
              CategoryMeta(id: 'medcare', label: '医療介護'),
              CategoryMeta(id: 'arxiv', label: 'arXiv'),
            ],
            topics: [TopicMeta(id: 1, name: '認知症')],
            sources: [],
          );
          final start = dateString(DateTime.now());
          h.store.digestFiles['day|$start'] = digestFile(
            body: '見出し: 認知症の新薬が国内で適用拡大へ\n- 要点その一\n- 要点その二',
          );
        },
      );
      expect(h.controller.ready, isTrue);
      expect(find.text('今日のダイジェスト'), findsOneWidget);
      expect(find.text('期間の概観'), findsOneWidget);
      expect(find.text('156件'), findsOneWidget);
      expect(find.text('arXiv'), findsWidgets);
      expect(find.text('話題の強さ'), findsOneWidget);
      expect(find.text('12件'), findsOneWidget);
      expect(find.text('+5'), findsOneWidget);
      expect(find.text('-2'), findsOneWidget);
      expect(find.text('±0'), findsOneWidget);
      expect(find.text('認知症の新薬が国内で適用拡大へ'), findsOneWidget);
      expect(find.text('要点その一'), findsOneWidget);
      expect(find.text('要点その二'), findsOneWidget);
      expect(find.text('1'), findsOneWidget);
      expect(find.text('2'), findsOneWidget);
      expect(find.text('claude · 記事1件'), findsOneWidget);
      // The image card shows the item title and source.
      expect(find.text('認知症の新薬'), findsOneWidget);
    });

    testWidgets('empty state and period switch', (tester) async {
      await pumpApp(tester);
      expect(find.text('今日のダイジェスト'), findsOneWidget);
      expect(find.text('この期間のダイジェストはまだありません'), findsOneWidget);
      await tester.tap(find.text('週'));
      await tester.pumpAndSettle();
      expect(find.text('今週のダイジェスト'), findsOneWidget);
      await tester.tap(find.text('月'));
      await tester.pumpAndSettle();
      expect(find.text('今月のダイジェスト'), findsOneWidget);
    });
  });

  group('feed (M-2) and tabs', () {
    testWidgets('lists items, unread count, search', (tester) async {
      await pumpApp(tester, items: sample());
      await tab(tester, 'feed');
      expect(find.text('認知症の新薬'), findsOneWidget);
      expect(find.text('介護報酬の改定'), findsOneWidget);
      expect(find.text('すべて · 未読 2件'), findsOneWidget);
      await tester.enterText(find.byType(TextField), '介護');
      await tester.pumpAndSettle();
      expect(find.text('認知症の新薬'), findsNothing);
      expect(find.text('介護報酬の改定'), findsOneWidget);
    });

    testWidgets('unread only and mark all read', (tester) async {
      final h = await pumpApp(tester, items: sample());
      await tab(tester, 'feed');
      await tester.tap(find.text('未読のみ'));
      await tester.pumpAndSettle();
      expect(find.text('既読の記事'), findsNothing);
      await tester.tap(find.byKey(const Key('mark-all-read')));
      await tester.pumpAndSettle();
      expect(h.store.pending.where((o) => o.field == Field.read), hasLength(2));
    });

    testWidgets('switching tabs shows each screen', (tester) async {
      await pumpApp(tester, items: sample());
      expect(find.text('今日のダイジェスト'), findsOneWidget);
      await tab(tester, 'feed');
      expect(find.text('記事を検索'), findsOneWidget);
      await tab(tester, 'starred');
      // Starred tab: only the starred item.
      expect(find.text('介護報酬の改定'), findsOneWidget);
      expect(find.text('認知症の新薬'), findsNothing);
      await tab(tester, 'settings');
      expect(find.text('設定'), findsWidgets);
      await tab(tester, 'digest');
      expect(find.text('今日のダイジェスト'), findsOneWidget);
    });

    testWidgets('pull to refresh runs one sync', (tester) async {
      final h = await pumpApp(tester, items: sample());
      await tab(tester, 'feed');
      expect(h.engine.runs, 0);
      await tester.fling(
        find.byType(ListView).first,
        const Offset(0, 400),
        1000,
      );
      await tester.pumpAndSettle();
      expect(h.engine.runs, 1);
    });
  });

  group('article (M-3)', () {
    Future<Harness> openFirst(WidgetTester tester) async {
      final h = await pumpApp(tester, items: sample());
      await tab(tester, 'feed');
      await tester.tap(find.text('認知症の新薬'));
      await tester.pumpAndSettle();
      return h;
    }

    testWidgets('opening an unread item marks it read', (tester) async {
      final h = await openFirst(tester);
      expect(find.text('新薬の要約です'), findsOneWidget);
      expect(find.text('記事を開く'), findsOneWidget);
      expect(
        h.store.pending.map((o) => (o.itemId, o.field, o.value)),
        contains((1, Field.read, true)),
      );
    });

    testWidgets('star toggle journals, read toggle journals', (tester) async {
      final h = await openFirst(tester);
      await tester.tap(find.byKey(const Key('star-button')));
      await tester.pumpAndSettle();
      expect(
        h.store.pending.map((o) => (o.itemId, o.field, o.value)),
        contains((1, Field.starred, true)),
      );
      await tester.tap(find.byKey(const Key('read-button')));
      await tester.pumpAndSettle();
      expect(
        h.store.pending.map((o) => (o.itemId, o.field, o.value)),
        contains((1, Field.read, false)),
      );
    });

    testWidgets('open uses the in-app browser; share copies the link', (
      tester,
    ) async {
      final h = await openFirst(tester);
      String? copied;
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        (call) async {
          if (call.method == 'Clipboard.setData') {
            copied = (call.arguments as Map)['text'] as String;
          }
          return null;
        },
      );
      await tester.tap(find.text('記事を開く'));
      await tester.pumpAndSettle();
      expect(h.opened, [Uri.parse('https://example.com/1')]);
      await tester.tap(find.byKey(const Key('share-button')));
      await tester.pumpAndSettle();
      expect(copied, 'https://example.com/1');
      expect(find.text('リンクをコピーしました'), findsOneWidget);
    });
  });

  group('first launch (M-5)', () {
    testWidgets('storage choice and Google login', (tester) async {
      final h = await pumpApp(
        tester,
        onboarded: false,
        setup: (h) => h.google.email = null,
      );
      expect(find.text('InfoDesk'), findsOneWidget);
      expect(find.text('Google ドライブ'), findsOneWidget);
      expect(find.text('NAS (WebDAV)'), findsOneWidget);
      expect(find.text('Google でログイン'), findsOneWidget);
      await tester.tap(find.text('Google でログイン'));
      await tester.pumpAndSettle();
      expect(h.google.signIns, 1);
      expect(h.controller.onboarded, isTrue);
      expect(find.text('今日のダイジェスト'), findsOneWidget);
    });

    testWidgets('NAS shows the guide, Tailscale login and the fields', (
      tester,
    ) async {
      final h = await pumpApp(
        tester,
        onboarded: false,
        setup: (h) => h.google.email = null,
      );
      await tester.tap(find.text('NAS (WebDAV)'));
      await tester.pumpAndSettle();
      expect(h.settings.current, Backend.nas);
      expect(find.text('NAS の準備'), findsOneWidget);
      expect(find.text('DSM で WebDAV Server を入れる'), findsOneWidget);
      expect(find.text('Tailscale にログイン'), findsOneWidget);
      expect(find.text('確かめる'), findsOneWidget);
      await tester.tap(find.text('Tailscale にログイン'));
      await tester.pumpAndSettle();
      expect(h.tailnet.calls, ['login']);
    });

    testWidgets('a good NAS check finishes setup', (tester) async {
      final h = await pumpApp(
        tester,
        onboarded: false,
        setup: (h) => h.google.email = null,
      );
      await tester.tap(find.text('NAS (WebDAV)'));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const Key('nas-url')),
        'http://nas:5005/x/',
      );
      await tester.enterText(find.byKey(const Key('nas-user')), 'infodesk');
      await tester.enterText(find.byKey(const Key('nas-password')), 'pw');
      await tester.tap(find.text('確かめる'));
      await tester.pumpAndSettle();
      expect(
        h.settings.calls,
        contains('setNas:http://nas:5005/x/:infodesk:pw'),
      );
      expect(h.storage.checks, 1);
      expect(h.controller.onboarded, isTrue);
    });
  });

  group('settings (M-4)', () {
    testWidgets('account, last sync, Mac update, version', (tester) async {
      final h = await pumpApp(
        tester,
        setup: (h) {
          h.store.settingsMap['last_sync_at'] = DateTime(
            2026,
            10,
            15,
            9,
            42,
          ).toUtc().toIso8601String();
          h.store.settingsMap['manifest.written_at'] = DateTime(
            2026,
            10,
            15,
            9,
            41,
          ).toUtc().toIso8601String();
          h.store.settingsMap['manifest.writer'] = 'MacBook Pro';
        },
      );
      await h.controller.init();
      await tab(tester, 'settings');
      expect(find.text('you@gmail.com'), findsOneWidget);
      expect(find.text('ログアウト'), findsOneWidget);
      expect(find.text('9:42'), findsOneWidget);
      expect(find.text('Mac からの更新'), findsOneWidget);
      expect(find.text('MacBook Pro · 9:41 に書き出し'), findsOneWidget);
      expect(find.text('今すぐ同期'), findsOneWidget);
      await tester.ensureVisible(find.text('バージョン', skipOffstage: false));
      expect(find.text('1.0.0', skipOffstage: false), findsOneWidget);
    });

    testWidgets('今すぐ同期 runs a pass', (tester) async {
      final h = await pumpApp(tester);
      await tab(tester, 'settings');
      await tester.tap(find.byKey(const Key('sync-now')));
      await tester.pumpAndSettle();
      expect(h.engine.runs, 1);
    });

    testWidgets('delete local copy asks first', (tester) async {
      final h = await pumpApp(tester, items: sample());
      await tab(tester, 'settings');
      await tester.ensureVisible(find.text('控えを削除', skipOffstage: false));
      await tester.pumpAndSettle();
      await tester.tap(find.text('控えを削除'));
      await tester.pumpAndSettle();
      expect(find.text('端末内の控えを消します。消しますか?'), findsOneWidget);
      expect(h.store.clearCalls, 0);
      await tester.tap(find.text('やめる'));
      await tester.pumpAndSettle();
      expect(h.store.clearCalls, 0);
      await tester.tap(find.text('控えを削除'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('消す'));
      await tester.pumpAndSettle();
      expect(h.store.clearCalls, 1);
    });

    Future<Harness> nasSettings(WidgetTester tester) async {
      final h = await pumpApp(
        tester,
        setup: (h) {
          h.settings.current = Backend.nas;
          h.settings.config = const NasConfig(
            url: 'http://nas:5005/x/',
            user: 'u',
            password: 'p',
          );
        },
      );
      await tab(tester, 'settings');
      return h;
    }

    testWidgets('tailscale progress shows the phase and elapsed seconds', (
      tester,
    ) async {
      final h = await nasSettings(tester);
      h.tailnet.state.value = TailnetStarting(
        phaseConnecting,
        h.clockNow.subtract(const Duration(seconds: 12)),
      );
      await tester.pump();
      expect(find.text('$phaseConnecting (12 秒)'), findsOneWidget);
      expect(find.text('時間がかかっています'), findsNothing);
      h.tailnet.state.value = const TailnetStopped();
      await tester.pumpAndSettle();
    });

    testWidgets('needs login shows the login button', (tester) async {
      final h = await nasSettings(tester);
      h.tailnet.state.value = TailnetNeedsLogin(Uri.parse('https://login'));
      await tester.pump();
      expect(find.text('ログインしてください'), findsOneWidget);
      await tester.tap(find.text('ログインページを開く'));
      await tester.pumpAndSettle();
      expect(h.tailnet.calls, ['login']);
    });

    testWidgets('slow start offers retry and a confirmed reset', (
      tester,
    ) async {
      final h = await nasSettings(tester);
      h.tailnet.state.value = TailnetStarting(
        phaseConnecting,
        h.clockNow.subtract(const Duration(seconds: 31)),
      );
      await tester.pump();
      expect(find.text('時間がかかっています'), findsOneWidget);
      await tester.tap(find.text('もう一度試す'));
      await tester.pump();
      expect(h.tailnet.calls, ['stop', 'start']);
      await tester.tap(find.text('状態を消してやり直す'));
      await tester.pump();
      expect(find.text('Tailscale にもう一度ログインが必要になります。消しますか?'), findsOneWidget);
      expect(h.tailnet.calls, isNot(contains('forget')));
      await tester.tap(find.text('消す'));
      await tester.pump();
      expect(h.tailnet.calls, contains('forget'));
      h.tailnet.state.value = const TailnetStopped();
      await tester.pumpAndSettle();
    });

    testWidgets('NAS check shows success and errors', (tester) async {
      final h = await nasSettings(tester);
      await tester.ensureVisible(find.text('確かめる', skipOffstage: false));
      await tester.pumpAndSettle();
      await tester.tap(find.text('確かめる'));
      await tester.pumpAndSettle();
      expect(find.text('読み取れました'), findsOneWidget);
      h.storage.checkResult = 'NAS のユーザー名かパスワードが違います';
      await tester.tap(find.text('確かめる'));
      await tester.pumpAndSettle();
      expect(find.text('NAS のユーザー名かパスワードが違います'), findsOneWidget);
    });
  });

  group('errors', () {
    testWidgets('a StorageError is shown in Japanese', (tester) async {
      final h = await pumpApp(
        tester,
        setup: (h) => h.engine.failWith = const StorageError(
          StorageErrorKind.retryable,
          networkMessage,
        ),
      );
      await h.controller.sync();
      await tester.pumpAndSettle();
      await tab(tester, 'settings');
      expect(find.text(networkMessage), findsOneWidget);
    });

    testWidgets('a raw exception never reaches the screen', (tester) async {
      final h = await pumpApp(
        tester,
        setup: (h) => h.engine.failWith = Exception('SocketException boom'),
      );
      await h.controller.sync();
      await tester.pumpAndSettle();
      await tab(tester, 'settings');
      expect(find.textContaining('boom'), findsNothing);
      expect(find.text('処理に失敗しました。もう一度お試しください'), findsOneWidget);
    });

    testWidgets('a failed tailscale start shows Japanese only', (tester) async {
      final h = await pumpApp(
        tester,
        setup: (h) => h.settings.current = Backend.nas,
      );
      await tab(tester, 'settings');
      h.tailnet.state.value = const TailnetFailed('dial tcp: lookup failed');
      await tester.pump();
      expect(find.textContaining('lookup'), findsNothing);
      expect(find.textContaining('Tailscale を起動できませんでした'), findsOneWidget);
      h.tailnet.state.value = const TailnetFailed(tailnetInitMessage);
      await tester.pump();
      expect(find.textContaining(tailnetInitMessage), findsOneWidget);
    });

    testWidgets('an invalid NAS URL shows the setting message', (tester) async {
      await pumpApp(tester, setup: (h) => h.settings.current = Backend.nas);
      await tab(tester, 'settings');
      await tester.enterText(find.byKey(const Key('nas-url')), 'https://nas/');
      await tester.tap(find.text('確かめる'));
      await tester.pumpAndSettle();
      expect(find.textContaining('http://'), findsWidgets);
    });
  });

  group('controller', () {
    test('setBackend waits for a running pass', () async {
      final h = Harness();
      final gate = Completer<void>();
      h.engine.hold = gate.future;
      await h.controller.init();
      final sync = h.controller.sync();
      await Future<void>.delayed(Duration.zero);
      final switched = h.controller.setBackend(Backend.nas);
      await Future<void>.delayed(const Duration(milliseconds: 20));
      expect(h.settings.calls, isEmpty);
      gate.complete();
      await sync;
      await switched;
      expect(h.settings.calls, ['setBackend:nas']);
    });

    test('a sync during a pass joins it', () async {
      final h = Harness();
      final gate = Completer<void>();
      h.engine.hold = gate.future;
      await h.controller.init();
      final a = h.controller.sync();
      final b = h.controller.sync();
      gate.complete();
      await Future.wait([a, b]);
      expect(h.engine.runs, 1);
    });
  });
}

import 'dart:async';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:infodesk_mobile/auth/google_account.dart';
import 'package:infodesk_mobile/auth/tailnet_service.dart';
import 'package:infodesk_mobile/data/local_store.dart';
import 'package:infodesk_mobile/settings/app_settings.dart';
import 'package:infodesk_mobile/storage/drive_store.dart';
import 'package:infodesk_mobile/storage/nas_store.dart';
import 'package:infodesk_mobile/storage/tailnet_port.dart';
import 'package:infodesk_mobile/storage/storage_factory.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';
import 'package:tailscale/tailscale.dart';

import '../auth/google_account_test.dart' show FakePort;
import '../settings/app_settings_test.dart'
    show ThrowingSecrets, readFailMessage;
import '../auth/tailnet_service_test.dart' show FakeNode, st;

void main() {
  sqfliteFfiInit();
  late LocalStore store;
  late AppSettings settings;
  late FakePort port;
  late FakeNode node;
  late TailnetService tailnet;
  late StorageFactory factory;
  late _Port port2;

  setUp(() async {
    store = await LocalStore.inMemory(factory: databaseFactoryFfi);
    settings = AppSettings(store, MemorySecretStore());
    port = FakePort();
    node = FakeNode();
    port2 = _Port();
    tailnet = TailnetService(node, openUrl: (_) async => true);
    factory = StorageFactory(
      settings: settings,
      google: GoogleAccount(port),
      tailnet: tailnet,
      nasPort: () => port2,
    );
  });
  tearDown(() => store.close());

  Future<String> failure() async {
    try {
      await factory.build();
    } on StorageError catch (e) {
      return e.message;
    }
    fail('expected a StorageError');
  }

  Future<void> fillNas({String? password = 'pw'}) => settings.setNas(
    url: 'http://nas:5005/infodesk/',
    user: 'kou',
    password: password,
  );

  group('Google Drive', () {
    test('not signed in asks for a new sign-in', () async {
      expect(await failure(), 'Google の再ログインが必要です');
    });

    test('signed in builds the Drive store', () async {
      port.clientValue = _client();
      expect(await factory.build(), isNotNull);
    });
  });

  group('NAS', () {
    setUp(() => settings.setBackend(Backend.nas));

    test('a launch start in flight is awaited before deciding', () async {
      await fillNas();
      node.saved = true;
      node.upGate = Completer<TailscaleStatus>();
      unawaited(tailnet.startOnLaunch(nas: true));
      final built = factory.build();
      await pumpEventQueue();
      node.upGate!.complete(st(NodeState.running));
      expect(await built, isA<NasStore>());
    });

    test('a start that stays Starting gives up after the bound', () async {
      await fillNas();
      node.saved = true;
      node.upGate = Completer<TailscaleStatus>();
      final quick = StorageFactory(
        settings: settings,
        google: GoogleAccount(port),
        tailnet: tailnet,
        nasPort: () => port2,
        tailnetWait: const Duration(milliseconds: 20),
      );
      unawaited(tailnet.start());
      await expectLater(quick.build(), throwsA(isA<StorageError>()));
      expect(tailnet.state.value, isA<TailnetStarting>());
    });

    test('stopped without saved state asks to log in to Tailscale', () async {
      expect(await failure(), 'Tailscale にログインしてください');
    });

    test('stopped with saved state says Tailscale is stopped', () async {
      node.saved = true;
      expect(await failure(), 'Tailscale が停止中です');
    });

    test('needs login asks to log in', () async {
      node.next = st(
        NodeState.needsLogin,
        url: 'https://login.tailscale.com/a/1',
      );
      await tailnet.start();
      expect(await failure(), 'Tailscale にログインしてください');
    });

    test('needs approval says so', () async {
      node.next = st(NodeState.needsMachineAuth);
      await tailnet.start();
      expect(await failure(), 'Tailscale の管理画面でこの端末を承認してください');
    });

    test('running but incomplete settings', () async {
      node.next = st(NodeState.running, ips: ['100.64.0.7']);
      await tailnet.start();
      expect(await failure(), 'NAS の URL、ユーザー名、パスワードを入力してください');
      await fillNas(password: null);
      expect(await failure(), 'NAS の URL、ユーザー名、パスワードを入力してください');
    });

    test('running and complete builds a NasStore', () async {
      node.next = st(NodeState.running, ips: ['100.64.0.7']);
      await tailnet.start();
      await fillNas();
      expect(await factory.build(), isA<NasStore>());
    });
  });

  group('secure storage failures', () {
    test('buildNas and checkNas report an unreadable password', () async {
      final secrets = ThrowingSecrets();
      final cfg = AppSettings(store, secrets);
      await cfg.setBackend(Backend.nas);
      await cfg.setNas(
        url: 'http://nas:5005/infodesk/',
        user: 'k',
        password: 'pw',
      );
      node.next = st(NodeState.running, ips: ['100.64.0.7']);
      await tailnet.start();
      final f = StorageFactory(
        settings: cfg,
        google: GoogleAccount(port),
        tailnet: tailnet,
        nasPort: () => port2,
      );
      secrets.failRead = true;
      await expectLater(
        f.buildNas(),
        throwsA(
          isA<StorageError>().having(
            (e) => e.message,
            'message',
            readFailMessage,
          ),
        ),
      );
      expect(await f.checkNas(), readFailMessage);
    });
  });

  group('checkNas', () {
    setUp(() async {
      await settings.setBackend(Backend.nas);
      node.next = st(NodeState.running, ips: ['100.64.0.7']);
      await tailnet.start();
      await fillNas();
    });

    test('lists the folder only and says it could be read', () async {
      port2.handler = (_) async => http.Response(
        '<?xml version="1.0"?><D:multistatus xmlns:D="DAV:">'
        '<D:response><D:href>/infodesk/</D:href><D:propstat><D:prop>'
        '<D:resourcetype><D:collection/></D:resourcetype></D:prop>'
        '<D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>'
        '</D:multistatus>',
        207,
        headers: {'content-type': 'application/xml; charset=utf-8'},
      );
      expect(await factory.checkNas(), '読み取れました');
      expect(port2.requests.map((r) => r.method), ['PROPFIND']);
    });

    test('returns the error message when the NAS cannot be read', () async {
      port2.handler = (_) async => http.Response('', 401);
      expect(await factory.checkNas(), 'NAS のユーザー名かパスワードが違います');
      expect(port2.requests.map((r) => r.method), ['PROPFIND']);
    });

    test('returns the factory message when Tailscale is not running', () async {
      await tailnet.logout();
      expect(await factory.checkNas(), 'Tailscale にログインしてください');
      expect(port2.requests, isEmpty);
    });

    test('works while Google is the selected backend', () async {
      await settings.setBackend(Backend.gdrive);
      port2.handler = (_) async => http.Response(
        '<D:multistatus xmlns:D="DAV:"/>',
        207,
        headers: {'content-type': 'application/xml'},
      );
      expect(await factory.checkNas(), '読み取れました');
    });
  });
}

class _Port implements TailnetPort {
  Future<http.Response> Function(http.Request) handler = (_) async =>
      http.Response('', 500);
  final requests = <http.Request>[];

  @override
  Future<http.Client> client() async => MockClient((r) {
    requests.add(r);
    return handler(r);
  });

  @override
  Future<InternetAddress?> peerAddr(String host) async =>
      host == 'nas' ? InternetAddress('100.64.0.7') : null;
}

http.Client _client() => _NoopClient();

class _NoopClient extends http.BaseClient {
  @override
  Future<http.StreamedResponse> send(http.BaseRequest request) =>
      throw UnimplementedError();
}

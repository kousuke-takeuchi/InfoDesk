import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/auth/tailnet_service.dart';
import 'package:tailscale/tailscale.dart';

class FakeNode implements TailnetNode {
  final events = StreamController<NodeState>.broadcast();
  TailscaleStatus next = TailscaleStatus.stopped;
  Object? upError;
  Completer<TailscaleStatus>? upGate;
  TailscaleStatus current = TailscaleStatus.stopped;
  bool saved = false;
  int ups = 0;
  int logouts = 0;
  int forgets = 0;
  int downs = 0;
  Object? logoutError;
  Object? forgetError;
  Object? downError;
  Object? statusError;

  /// Errors `up` throws first, one per call, before it succeeds.
  final upErrors = <Object>[];

  @override
  Stream<NodeState> get onStateChange => events.stream;

  @override
  Future<TailscaleStatus> up(String hostname) async {
    ups++;
    if (upErrors.isNotEmpty) throw upErrors.removeAt(0);
    final e = upError;
    if (e != null) throw e;
    final g = upGate;
    final s = g != null ? await g.future : next;
    current = s;
    return s;
  }

  @override
  Future<TailscaleStatus> status() async {
    final e = statusError;
    if (e != null) throw e;
    return current;
  }

  @override
  Future<void> logout() async {
    logouts++;
    final e = logoutError;
    if (e != null) throw e;
    // Like tailscale 0.9.0: logout keeps the local state.
  }

  @override
  Future<void> forgetLocalIdentity() async {
    forgets++;
    final e = forgetError;
    if (e != null) throw e;
    saved = false;
  }

  @override
  Future<void> down() async {
    downs++;
    final e = downError;
    if (e != null) throw e;
  }

  @override
  Future<bool> hasSavedState() async => saved;
}

TailscaleStatus st(NodeState s, {String? url, List<String> ips = const []}) =>
    TailscaleStatus(
      state: s,
      authUrl: url == null ? null : Uri.parse(url),
      tailscaleIPs: ips,
      health: const [],
    );

void main() {
  late FakeNode node;
  late List<Uri> opened;
  late DateTime now;
  late TailnetService svc;

  setUp(() {
    node = FakeNode();
    opened = [];
    now = DateTime.utc(2026, 10, 15, 12);
    svc = TailnetService(
      node,
      openUrl: (u) async {
        opened.add(u);
        return true;
      },
      clock: () => now,
    );
  });

  test('starts stopped', () {
    expect(svc.state.value, isA<TailnetStopped>());
  });

  test('start goes Starting then Running and never opens the URL', () async {
    node.upGate = Completer();
    final f = svc.start();
    await Future<void>.delayed(Duration.zero);
    final s = svc.state.value as TailnetStarting;
    expect(s.phase, 'Tailscale のサーバーにつないでいます');
    expect(s.startedAt, now);
    node.upGate!.complete(st(NodeState.running, ips: ['100.64.0.9']));
    await f;
    final r = svc.state.value as TailnetRunning;
    expect(r.name, 'infodesk-mobile');
    expect(r.ip, '100.64.0.9');
    expect(opened, isEmpty);
  });

  test(
    'start with no credentials gives NeedsLogin and does not open the URL',
    () async {
      node.next = st(
        NodeState.needsLogin,
        url: 'https://login.tailscale.com/a/1',
      );
      await svc.start();
      final s = svc.state.value as TailnetNeedsLogin;
      expect(s.url, Uri.parse('https://login.tailscale.com/a/1'));
      expect(opened, isEmpty);
    },
  );

  test(
    'login opens the URL only when called, and again without restarting',
    () async {
      node.next = st(
        NodeState.needsLogin,
        url: 'https://login.tailscale.com/a/1',
      );
      await svc.start();
      expect(opened, isEmpty);
      await svc.login();
      expect(opened, [Uri.parse('https://login.tailscale.com/a/1')]);
      expect(node.ups, 1, reason: 'login from NeedsLogin does not restart');
      await svc.login();
      expect(opened.length, 2);
    },
  );

  test('login from stopped starts the node then opens the URL', () async {
    node.next = st(
      NodeState.needsLogin,
      url: 'https://login.tailscale.com/a/2',
    );
    await svc.login();
    expect(node.ups, 1);
    expect(opened, [Uri.parse('https://login.tailscale.com/a/2')]);
  });

  test('login waits for a URL that arrives later via onStateChange', () async {
    node.next = st(NodeState.needsLogin); // no URL yet
    await svc.login();
    expect((svc.state.value as TailnetStarting).phase, 'ログインの準備をしています');
    expect(opened, isEmpty);
    node.current = st(
      NodeState.needsLogin,
      url: 'https://login.tailscale.com/a/3',
    );
    node.events.add(NodeState.needsLogin);
    await Future<void>.delayed(Duration.zero);
    await Future<void>.delayed(Duration.zero);
    expect(svc.state.value, isA<TailnetNeedsLogin>());
    expect(opened, [Uri.parse('https://login.tailscale.com/a/3')]);
  });

  test(
    'a URL that arrives later is not opened unless login was pressed',
    () async {
      node.next = st(NodeState.needsLogin);
      await svc.start();
      node.current = st(
        NodeState.needsLogin,
        url: 'https://login.tailscale.com/a/3',
      );
      node.events.add(NodeState.needsLogin);
      await Future<void>.delayed(Duration.zero);
      await Future<void>.delayed(Duration.zero);
      expect(svc.state.value, isA<TailnetNeedsLogin>());
      expect(opened, isEmpty);
    },
  );

  test('state changes after login completes lead to Running', () async {
    node.next = st(
      NodeState.needsLogin,
      url: 'https://login.tailscale.com/a/1',
    );
    await svc.start();
    node.current = st(NodeState.running, ips: ['100.64.0.9']);
    node.events.add(NodeState.running);
    await Future<void>.delayed(Duration.zero);
    await Future<void>.delayed(Duration.zero);
    expect(svc.state.value, isA<TailnetRunning>());
  });

  test('NeedsMachineAuth is NeedsApproval', () async {
    node.next = st(NodeState.needsMachineAuth);
    await svc.start();
    expect(svc.state.value, isA<TailnetNeedsApproval>());
  });

  test('a start failure is Failed with a reason', () async {
    node.upError = const TailscaleUpException('timeout');
    await svc.start();
    final f = svc.state.value as TailnetFailed;
    expect(f.reason, contains('timeout'));
    expect(opened, isEmpty);
  });

  test('a Stopped event while running makes the state Stopped', () async {
    node.next = st(NodeState.running, ips: ['100.64.0.9']);
    await svc.start();
    node.current = TailscaleStatus.stopped;
    node.events.add(NodeState.stopped);
    await Future<void>.delayed(Duration.zero);
    await Future<void>.delayed(Duration.zero);
    expect(svc.state.value, isA<TailnetStopped>());
  });

  test('start while starting or running does nothing', () async {
    node.next = st(NodeState.running, ips: ['100.64.0.9']);
    await svc.start();
    await svc.start();
    expect(node.ups, 1);
  });

  test(
    'logout stops and returns to Stopped; a failure is shown as Failed',
    () async {
      node.next = st(NodeState.running, ips: ['100.64.0.9']);
      await svc.start();
      await svc.logout();
      expect(node.logouts, 1);
      expect(svc.state.value, isA<TailnetStopped>());

      node.logoutError = const TailscaleLogoutException('no network');
      await svc.logout();
      expect(svc.state.value, isA<TailnetFailed>());
    },
  );

  test('forget clears the local identity without logout', () async {
    node.next = st(NodeState.running, ips: ['100.64.0.9']);
    await svc.start();
    await svc.forget();
    expect(node.forgets, 1);
    expect(node.logouts, 0);
    expect(svc.state.value, isA<TailnetStopped>());
  });

  group('logout forgets the identity', () {
    test(
      'a successful logout also forgets, so hasSavedState is false',
      () async {
        node.saved = true;
        node.next = st(NodeState.running, ips: ['100.64.0.9']);
        await svc.start();
        await svc.logout();
        expect(node.logouts, 1);
        expect(node.forgets, 1);
        expect(await svc.hasSavedState(), isFalse);
        expect(svc.state.value, isA<TailnetStopped>());
      },
    );

    test('a failed logout keeps the identity', () async {
      node.saved = true;
      node.logoutError = const TailscaleLogoutException('no network');
      await svc.logout();
      expect(node.forgets, 0);
      expect(await svc.hasSavedState(), isTrue);
      expect(svc.state.value, isA<TailnetFailed>());
    });

    test('a failed forget after logout is Failed', () async {
      node.forgetError = const TailscaleForgetLocalIdentityException('busy');
      await svc.logout();
      expect(svc.state.value, isA<TailnetFailed>());
    });
  });

  group('the login flag does not go stale', () {
    final url = 'https://login.tailscale.com/a/9';

    test('login while Running does not arm the browser', () async {
      node.next = st(NodeState.running, ips: ['100.64.0.9']);
      await svc.start();
      await svc.login();
      expect(opened, isEmpty);
      await svc.stop();
      node.next = st(NodeState.needsLogin, url: url);
      await svc.start();
      expect(svc.state.value, isA<TailnetNeedsLogin>());
      expect(opened, isEmpty);
    });

    test('login while NeedsApproval does not arm the browser', () async {
      node.next = st(NodeState.needsMachineAuth);
      await svc.start();
      await svc.login();
      await svc.stop();
      node.next = st(NodeState.needsLogin, url: url);
      await svc.start();
      expect(opened, isEmpty);
    });

    test('a failed start after login clears the flag', () async {
      node.upError = const TailscaleUpException('timeout');
      await svc.login();
      expect(svc.state.value, isA<TailnetFailed>());
      node.upError = null;
      node.next = st(NodeState.needsLogin, url: url);
      await svc.start();
      expect(svc.state.value, isA<TailnetNeedsLogin>());
      expect(opened, isEmpty);
    });

    test('a non-Tailscale start failure clears the flag too', () async {
      node.upError = StateError('boom');
      await svc.login();
      expect(svc.state.value, isA<TailnetFailed>());
      node.upError = null;
      node.next = st(NodeState.needsLogin, url: url);
      await svc.start();
      expect(opened, isEmpty);
    });
  });

  group('state key errors', () {
    const hint = 'Tailscale の状態を読めません。「状態を消してやり直す」を押してください';
    TailscaleUpException up(TailscaleErrorCode c) =>
        TailscaleUpException('x', code: c);

    test('missingStateKey shows the hint and does not retry', () async {
      node.upError = up(TailscaleErrorCode.missingStateKey);
      await svc.start();
      expect((svc.state.value as TailnetFailed).reason, hint);
      expect(node.forgets, 0);
      expect(node.ups, 1);
    });

    test('orphanedStateKey forgets and retries the start once', () async {
      node.upErrors.add(up(TailscaleErrorCode.orphanedStateKey));
      node.next = st(
        NodeState.needsLogin,
        url: 'https://login.tailscale.com/a/1',
      );
      await svc.start();
      expect(node.forgets, 1);
      expect(node.ups, 2);
      expect(svc.state.value, isA<TailnetNeedsLogin>());
      expect(opened, isEmpty);
    });

    test('a second orphanedStateKey is the hint, no more retries', () async {
      node.upError = up(TailscaleErrorCode.orphanedStateKey);
      await svc.start();
      expect(node.forgets, 1);
      expect(node.ups, 2);
      expect((svc.state.value as TailnetFailed).reason, hint);
    });

    test('a failing forget during the retry is Failed', () async {
      node.upErrors.add(up(TailscaleErrorCode.orphanedStateKey));
      node.forgetError = const TailscaleForgetLocalIdentityException('busy');
      await svc.start();
      expect(node.ups, 1);
      expect(svc.state.value, isA<TailnetFailed>());
    });
  });

  group('non-Tailscale errors are caught', () {
    test('stop with a failing down is Failed', () async {
      node.downError = StateError('boom');
      await svc.stop();
      expect(svc.state.value, isA<TailnetFailed>());
    });

    test('a status failure after an event is Failed', () async {
      node.next = st(NodeState.running, ips: ['100.64.0.9']);
      await svc.start();
      node.statusError = StateError('boom');
      node.events.add(NodeState.running);
      await Future<void>.delayed(Duration.zero);
      await Future<void>.delayed(Duration.zero);
      expect(svc.state.value, isA<TailnetFailed>());
    });
  });

  group('startOnLaunch', () {
    test('starts only for the NAS backend with saved state', () async {
      node.next = st(NodeState.running, ips: ['100.64.0.9']);
      node.saved = false;
      await svc.startOnLaunch(nas: true);
      expect(node.ups, 0);
      node.saved = true;
      await svc.startOnLaunch(nas: false);
      expect(node.ups, 0);
      await svc.startOnLaunch(nas: true);
      expect(node.ups, 1);
      expect(opened, isEmpty);
    });
  });

  group('text', () {
    test('Japanese status text with elapsed seconds', () {
      final t0 = DateTime.utc(2026, 10, 15, 12);
      final later = t0.add(const Duration(seconds: 7, milliseconds: 900));
      expect(
        tailnetText(TailnetStarting('Tailscale のサーバーにつないでいます', t0), later),
        'Tailscale のサーバーにつないでいます (7 秒)',
      );
      expect(tailnetText(const TailnetStopped(), later), '停止中');
      expect(
        tailnetText(TailnetNeedsLogin(Uri.parse('https://x')), later),
        'ログインしてください',
      );
      expect(
        tailnetText(const TailnetNeedsApproval(), later),
        'Tailscale の管理画面でこの端末を承認してください',
      );
      expect(
        tailnetText(const TailnetRunning('infodesk-mobile', '1.2.3.4'), later),
        '接続済み · infodesk-mobile',
      );
      expect(tailnetText(const TailnetFailed('x'), later), '起動できませんでした: x');
    });
  });
}

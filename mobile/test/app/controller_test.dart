import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/app/controller.dart';
import 'package:infodesk_mobile/auth/tailnet_service.dart';
import 'package:infodesk_mobile/settings/app_settings.dart';
import 'package:infodesk_mobile/storage/drive_store.dart';
import 'package:infodesk_mobile/storage/storage_factory.dart';
import 'package:infodesk_mobile/storage/tailnet_port.dart';
import 'package:infodesk_mobile/sync/engine.dart';
import 'package:tailscale/tailscale.dart';

import '../auth/tailnet_service_test.dart' show FakeNode, st;
import '../ui/fakes.dart';

/// A pass that only builds the store, like the real engine's first step.
class BuildingEngine implements SyncEngine {
  BuildingEngine(this.storage);
  final StorageFactory storage;
  int runs = 0;
  int built = 0;

  @override
  Future<SyncReport> runOnce({DateTime? now}) async {
    runs++;
    await storage.build();
    built++;
    return SyncReport();
  }

  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
}

class _Port implements TailnetPort {
  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
}

void main() {
  group('NAS launch', () {
    late FakeNode node;
    late TailnetService tailnet;
    late FakeSettings settings;
    late BuildingEngine engine;
    late AppController c;

    AppController build(Duration wait) {
      node = FakeNode()
        ..saved = true
        ..upGate = Completer<TailscaleStatus>();
      tailnet = TailnetService(node, openUrl: (_) async => true);
      settings = FakeSettings()
        ..current = Backend.nas
        ..config = const NasConfig(
          url: 'http://nas:5005/infodesk/',
          user: 'kou',
          password: 'pw',
        );
      final store = FakeStore()..settingsMap['onboarded'] = 'true';
      final google = FakeGoogle();
      final storage = StorageFactory(
        settings: settings,
        google: google,
        tailnet: tailnet,
        nasPort: _Port.new,
        tailnetWait: wait,
      );
      engine = BuildingEngine(storage);
      return c = AppController(
        store: store,
        engine: engine,
        settings: settings,
        storage: storage,
        google: google,
        tailnet: tailnet,
        openArticle: (_) async => true,
        openExternal: (_) async => true,
      );
    }

    test('the open-time pass waits for a slow start and succeeds', () async {
      build(const Duration(seconds: 30));
      final started = c.start();
      await pumpEventQueue();
      expect(engine.built, 0, reason: 'still waiting for Tailscale');
      node.upGate!.complete(st(NodeState.running));
      await started;
      await pumpEventQueue();
      while (c.syncing) {
        await pumpEventQueue();
      }
      expect(engine.built, 1);
      expect(c.syncError, isNull);
      expect(c.lastSyncAt, isNotNull);
    });

    test('a start slower than the wait syncs again once Running', () async {
      build(const Duration(milliseconds: 20));
      await c.start();
      await Future<void>.delayed(const Duration(milliseconds: 60));
      await pumpEventQueue();
      expect(engine.runs, 1);
      expect(c.syncError, isNotNull, reason: 'the first pass gave up');
      node.upGate!.complete(st(NodeState.running));
      await pumpEventQueue();
      while (c.syncing) {
        await pumpEventQueue();
      }
      expect(engine.runs, 2);
      expect(engine.built, 1);
      expect(c.syncError, isNull, reason: 'no false error left showing');
    });

    test('Running without a Tailscale failure does not sync again', () async {
      build(const Duration(seconds: 30));
      unawaited(c.start());
      await pumpEventQueue();
      node.upGate!.complete(st(NodeState.running));
      await pumpEventQueue();
      while (c.syncing) {
        await pumpEventQueue();
      }
      expect(engine.runs, 1);
      tailnet.state.value = const TailnetStopped();
      tailnet.state.value = TailnetRunning(tailnetHostname, null);
      await pumpEventQueue();
      expect(engine.runs, 1);
    });
  });

  group('launch', () {
    test('start() with onboarded runs one pass', () async {
      final h = Harness();
      await h.controller.start();
      await pumpEventQueue();
      expect(h.engine.runs, 1);
    });

    test('start() before setup runs no pass', () async {
      final h = Harness(onboarded: false);
      await h.controller.start();
      await pumpEventQueue();
      expect(h.engine.runs, 0);
    });

    test('the Google restore never holds up the first screen', () async {
      final h = Harness();
      final hold = Completer<void>();
      h.google.restoreHold = hold.future;
      final done = h.controller.init();
      await pumpEventQueue();
      expect(h.controller.ready, isTrue);
      expect(h.google.restores, 1);
      hold.complete();
      await done;
    });

    test('a pass failing with a network error is not a re-login', () async {
      final h = Harness();
      h.engine.failWith = const StorageError(
        StorageErrorKind.retryable,
        networkMessage,
      );
      await h.controller.start();
      await pumpEventQueue();
      expect(h.controller.authExpired, isFalse);
      expect(h.controller.syncError, networkMessage);
    });
  });
}

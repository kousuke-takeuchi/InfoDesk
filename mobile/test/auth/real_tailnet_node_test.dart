import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/auth/backup_exclusion.dart';
import 'package:infodesk_mobile/auth/tailnet_service.dart';
import 'package:tailscale/tailscale.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  late Directory base;
  late List<MethodCall> calls;
  Object? Function(MethodCall)? reply;

  setUp(() async {
    base = await Directory.systemTemp.createTemp('tailnet_test');
    calls = [];
    reply = (_) => true;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel(backupChannelName), (
          c,
        ) async {
          calls.add(c);
          return reply!(c);
        });
    RealTailnetNode.resetForTest();
  });
  tearDown(() async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel(backupChannelName), null);
    await base.delete(recursive: true);
  });

  test('BackupExclusion asks the iOS side to exclude the path', () async {
    await BackupExclusion(isIOS: true).exclude('/x/tailnet');
    expect(calls.single.method, 'exclude');
    expect(calls.single.arguments, '/x/tailnet');
  });

  test('BackupExclusion fails when the native side says no', () async {
    reply = (_) => false;
    await expectLater(
      BackupExclusion(isIOS: true).exclude('/x'),
      throwsA(anything),
    );
    reply = (_) => throw PlatformException(code: 'e');
    await expectLater(
      BackupExclusion(isIOS: true).exclude('/x'),
      throwsA(anything),
    );
  });

  test('BackupExclusion does nothing off iOS', () async {
    await BackupExclusion(isIOS: false).exclude('/x');
    expect(calls, isEmpty);
  });

  test('create excludes the state directory before Tailscale.init', () async {
    final order = <String>[];
    reply = (c) {
      order.add('exclude');
      return true;
    };
    await expectLater(
      RealTailnetNode.create(
        supportDir: () async => base,
        backup: BackupExclusion(isIOS: true),
        init: ({required stateDir, required appId}) {
          order.add('init');
          throw const TailscaleConfigurationException('frozen');
        },
      ),
      throwsA(isA<TailnetInitError>()),
    );
    expect(order, ['exclude', 'init']);
    expect(calls.single.arguments, '${base.path}/tailnet');
  });

  test('a Tailscale.init failure becomes a Japanese message', () async {
    await expectLater(
      RealTailnetNode.create(
        supportDir: () async => base,
        backup: BackupExclusion(isIOS: false),
        init: ({required stateDir, required appId}) =>
            throw const TailscaleUsageException('no fd'),
      ),
      throwsA(
        isA<TailnetInitError>().having(
          (e) => e.message,
          'message',
          tailnetInitMessage,
        ),
      ),
    );
  });

  test('a failed exclusion stops the start before init', () async {
    reply = (_) => false;
    var inited = false;
    await expectLater(
      RealTailnetNode.create(
        supportDir: () async => base,
        backup: BackupExclusion(isIOS: true),
        init: ({required stateDir, required appId}) => inited = true,
      ),
      throwsA(
        isA<TailnetInitError>().having(
          (e) => e.message,
          'message',
          tailnetBackupMessage,
        ),
      ),
    );
    expect(inited, isFalse);
  });
}

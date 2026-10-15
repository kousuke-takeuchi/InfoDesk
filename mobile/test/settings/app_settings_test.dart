import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/data/local_store.dart';
import 'package:infodesk_mobile/settings/app_settings.dart';
import 'package:infodesk_mobile/sync/format.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';

import '../data/local_store_test.dart' show item;

const urlMsg = 'NAS の URL を確かめてください (例: http://takeuchi-nas:5005/infodesk/)';
const httpsMsg = 'http:// を使ってください (Tailscale の中なので暗号化されています)';

void main() {
  sqfliteFfiInit();

  group('validateNasUrl', () {
    test('accepts http with a host, trimmed', () {
      expect(validateNasUrl('http://takeuchi-nas:5005/infodesk/'), isNull);
      expect(validateNasUrl(' http://takeuchi-nas:5005/infodesk/ '), isNull);
      expect(validateNasUrl('http://100.100.1.5/a'), isNull);
    });

    test('rejects https with the encryption hint', () {
      expect(validateNasUrl('https://nas/infodesk/'), httpsMsg);
    });

    test('rejects everything else with the example message', () {
      for (final bad in [
        '',
        'takeuchi-nas:5005/infodesk/',
        'ftp://nas/infodesk/',
        'file:///tmp/x',
        'http://',
        'http://kou:pw@nas:5005/infodesk/',
        'http://kou@nas:5005/infodesk/',
        'not a url',
      ]) {
        expect(validateNasUrl(bad), urlMsg, reason: bad);
      }
    });
  });

  group('AppSettings', () {
    late LocalStore store;
    late MemorySecretStore secrets;
    late AppSettings settings;

    setUp(() async {
      store = await LocalStore.inMemory(factory: databaseFactoryFfi);
      secrets = MemorySecretStore();
      settings = AppSettings(store, secrets);
    });
    tearDown(() => store.close());

    test(
      'backend defaults to gdrive and any other value reads as gdrive',
      () async {
        expect(await settings.backend(), Backend.gdrive);
        await store.setSetting('backend', 'google');
        expect(await settings.backend(), Backend.gdrive);
        await store.setSetting('backend', 'nas');
        expect(await settings.backend(), Backend.nas);
      },
    );

    test(
      'switching backend wipes the local copy and synced-file records',
      () async {
        await store.upsertDay(DayFile(date: '2026-10-10', items: [item(1)]));
        await store.putSyncedFile('day-2026-10-10.json.gz', 'h', 'day');
        expect(await store.items(), isNotEmpty);
        expect(await store.syncedFile('day-2026-10-10.json.gz'), isNotNull);

        expect(await settings.setBackend(Backend.nas), isTrue);

        expect(await settings.backend(), Backend.nas);
        expect(await store.items(), isEmpty);
        expect(await store.syncedFile('day-2026-10-10.json.gz'), isNull);
      },
    );

    test('choosing the current backend again keeps the local copy', () async {
      await settings.setBackend(Backend.nas);
      await store.upsertDay(DayFile(date: '2026-10-10', items: [item(1)]));
      expect(await settings.setBackend(Backend.nas), isFalse);
      expect(await store.items(), isNotEmpty);
    });

    test('NAS password goes to secure storage only', () async {
      await settings.setNas(
        url: ' http://nas:5005/x/ ',
        user: 'kou',
        password: 'secret-pw',
      );
      final nas = await settings.nas();
      expect(nas.url, 'http://nas:5005/x/');
      expect(nas.user, 'kou');
      expect(nas.password, 'secret-pw');
      expect(nas.complete, isTrue);
      expect(secrets.values, {'nas.password': 'secret-pw'});
      expect(await store.getSetting('nas.url'), 'http://nas:5005/x/');
      expect(await store.getSetting('nas.user'), 'kou');
      // Nothing in the database holds the password.
      for (final k in ['nas.password', 'nas.url', 'nas.user', 'backend']) {
        expect(await store.getSetting(k), isNot('secret-pw'));
      }
    });

    test(
      'a null password keeps the saved one, an empty one removes it',
      () async {
        await settings.setNas(url: 'http://nas/a/', user: 'k', password: 'pw');
        await settings.setNas(url: 'http://nas/a/', user: 'k2');
        expect((await settings.nas()).password, 'pw');
        expect((await settings.nas()).user, 'k2');
        await settings.setNas(url: 'http://nas/a/', user: 'k2', password: '');
        expect((await settings.nas()).password, isNull);
        expect((await settings.nas()).complete, isFalse);
      },
    );

    test('a bad URL saves nothing', () async {
      expect(
        () => settings.setNas(url: 'https://nas/', user: 'k', password: 'pw'),
        throwsA(
          isA<InvalidSetting>().having((e) => e.message, 'message', httpsMsg),
        ),
      );
      await Future<void>.delayed(Duration.zero);
      expect(secrets.values, isEmpty);
      expect(await store.getSetting('nas.url'), isNull);
    });

    test('an empty URL is allowed (not entered yet)', () async {
      await settings.setNas(url: '', user: 'k', password: 'pw');
      expect((await settings.nas()).complete, isFalse);
    });

    test(
      'changing the URL while NAS is the backend wipes the local copy',
      () async {
        await settings.setBackend(Backend.nas);
        await settings.setNas(url: 'http://nas/a/', user: 'k', password: 'pw');
        await store.upsertDay(DayFile(date: '2026-10-10', items: [item(1)]));
        await settings.setNas(url: 'http://nas/a/', user: 'k2');
        expect(await store.items(), isNotEmpty);
        await settings.setNas(url: 'http://other/b/', user: 'k2');
        expect(await store.items(), isEmpty);
      },
    );

    test(
      'clearNas removes url, user and password but not the backend',
      () async {
        await settings.setBackend(Backend.nas);
        await settings.setNas(url: 'http://nas/a/', user: 'k', password: 'pw');
        await settings.clearNas();
        final nas = await settings.nas();
        expect([nas.url, nas.user, nas.password], ['', '', null]);
        expect(secrets.values, isEmpty);
        expect(await settings.backend(), Backend.nas);
      },
    );
  });
}

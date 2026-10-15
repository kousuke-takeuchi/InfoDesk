import 'package:flutter_test/flutter_test.dart';
import 'package:google_sign_in/google_sign_in.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:infodesk_mobile/auth/google_account.dart';
import 'package:infodesk_mobile/storage/drive_store.dart';
import 'package:infodesk_mobile/storage/google_drive_store.dart';

class FakePort implements GoogleAuthPort {
  String? restored;
  String? email = 'me@example.com';
  Object? signInError;
  http.Client? clientValue;
  Object? restoreError;
  Object? clientError;
  int signOuts = 0;

  @override
  Future<String?> restore() async {
    final e = restoreError;
    if (e != null) throw e;
    return restored;
  }

  @override
  Future<String> signIn() async {
    final e = signInError;
    if (e != null) throw e;
    return email!;
  }

  @override
  Future<http.Client?> client() async {
    final e = clientError;
    if (e != null) throw e;
    return clientValue;
  }

  @override
  Future<void> signOut() async => signOuts++;
}

void main() {
  late FakePort port;
  late GoogleAccount account;
  setUp(() {
    port = FakePort();
    account = GoogleAccount(port);
  });

  test('the scopes are drive.appdata, openid and email', () {
    expect(googleScopes, [
      'https://www.googleapis.com/auth/drive.appdata',
      'openid',
      'email',
    ]);
  });

  test('signIn stores the email, signOut clears it', () async {
    expect(account.currentEmail, isNull);
    expect(await account.signIn(), 'me@example.com');
    expect(account.currentEmail, 'me@example.com');
    await account.signOut();
    expect(port.signOuts, 1);
    expect(account.currentEmail, isNull);
  });

  test('restore picks up an existing session', () async {
    port.restored = 'back@example.com';
    await account.restore();
    expect(account.currentEmail, 'back@example.com');
  });

  test('cancelling the sign-in returns null', () async {
    port.signInError = const GoogleSignInException(
      code: GoogleSignInExceptionCode.canceled,
    );
    expect(await account.signIn(), isNull);
    expect(account.currentEmail, isNull);
  });

  test('other sign-in failures become a Japanese StorageError', () async {
    port.signInError = const GoogleSignInException(
      code: GoogleSignInExceptionCode.clientConfigurationError,
    );
    await expectLater(
      account.signIn(),
      throwsA(
        isA<StorageError>().having(
          (e) => e.message,
          'message',
          'Google にログインできませんでした',
        ),
      ),
    );
  });

  test(
    'driveStore asks for a new sign-in when the authorization is gone',
    () async {
      port.clientValue = null;
      await expectLater(
        account.driveStore(),
        throwsA(
          isA<StorageError>()
              .having((e) => e.kind, 'kind', StorageErrorKind.authExpired)
              .having((e) => e.message, 'message', reauthMessage),
        ),
      );
    },
  );

  test('driveStore returns a GoogleDriveStore when authorized', () async {
    port.clientValue = MockClient((_) async => http.Response('{}', 200));
    expect(await account.driveStore(), isA<GoogleDriveStore>());
  });

  test('restore swallows every error', () async {
    port.restoreError = StateError('keychain locked');
    await account.restore();
    expect(account.currentEmail, isNull);
    port.restoreError = const GoogleSignInException(
      code: GoogleSignInExceptionCode.unknownError,
    );
    await account.restore();
    expect(account.currentEmail, isNull);
  });

  test('raw Google errors from client() become the re-login error', () async {
    for (final e in <Object>[
      const GoogleSignInException(code: GoogleSignInExceptionCode.interrupted),
      StateError('boom'),
    ]) {
      port.clientError = e;
      await expectLater(
        account.driveStore(),
        throwsA(
          isA<StorageError>()
              .having((x) => x.kind, 'kind', StorageErrorKind.authExpired)
              .having((x) => x.message, 'message', reauthMessage),
        ),
      );
    }
  });

  test('a StorageError from client() passes through unchanged', () async {
    port.clientError = const StorageError(StorageErrorKind.other, 'x');
    await expectLater(
      account.driveStore(),
      throwsA(
        isA<StorageError>().having(
          (x) => x.kind,
          'kind',
          StorageErrorKind.other,
        ),
      ),
    );
  });

  test(
    'LazyInit retries after a failure and runs once after success',
    () async {
      var n = 0;
      final init = LazyInit(() async {
        n++;
        if (n == 1) throw StateError('first');
      });
      await expectLater(init.ready(), throwsStateError);
      await init.ready();
      await init.ready();
      expect(n, 2);
    },
  );
}

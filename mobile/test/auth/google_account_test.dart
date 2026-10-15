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
  int signOuts = 0;

  @override
  Future<String?> restore() async => restored;

  @override
  Future<String> signIn() async {
    final e = signInError;
    if (e != null) throw e;
    return email!;
  }

  @override
  Future<http.Client?> client() async => clientValue;

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
}

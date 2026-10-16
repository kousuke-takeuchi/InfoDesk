import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:google_sign_in/google_sign_in.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:infodesk_mobile/auth/google_account.dart';
import 'package:infodesk_mobile/storage/drive_store.dart';
import 'package:infodesk_mobile/storage/google_drive_store.dart';

class FakePort implements GoogleAuthPort {
  String? restored;

  /// When set, restore waits for it.
  Future<void>? restoreHold;
  String? email = 'me@example.com';
  Object? signInError;
  http.Client? clientValue;
  Object? restoreError;
  Object? clientError;
  int signOuts = 0;

  @override
  Future<String?> restore() async {
    await restoreHold;
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

  @override
  String? get currentEmail => email;
}

/// Stands in for `GoogleSignIn.instance`. [lightweight] answers each
/// attemptLightweightAuthentication in turn: an account, null, or an error.
class FakeGs implements GoogleSignIn {
  final lightweight = <Object?>[];
  int attempts = 0;

  /// When set, each lightweight attempt waits for it.
  Future<void>? gate;

  @override
  Future<void> signOut() async {}

  @override
  Future<void> initialize({
    String? clientId,
    String? serverClientId,
    String? nonce,
    String? hostedDomain,
  }) async {}

  @override
  Future<GoogleSignInAccount?>? attemptLightweightAuthentication({
    bool reportAllExceptions = false,
  }) async {
    attempts++;
    await gate;
    final r = lightweight.isEmpty ? null : lightweight.removeAt(0);
    if (r is GoogleSignInAccount?) return r;
    throw r;
  }

  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
}

class FakeAuthz implements GoogleSignInAuthorizationClient {
  Object? error;
  bool granted = true;

  @override
  Future<GoogleSignInClientAuthorization?> authorizationForScopes(
    List<String> scopes,
  ) async {
    final e = error;
    if (e != null) throw e;
    return granted
        ? const GoogleSignInClientAuthorization(accessToken: 'tok')
        : null;
  }

  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
}

class FakeAccount implements GoogleSignInAccount {
  FakeAccount(this.email);
  @override
  final String email;
  @override
  final FakeAuthz authorizationClient = FakeAuthz();

  @override
  dynamic noSuchMethod(Invocation i) =>
      throw UnimplementedError('${i.memberName}');
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

  group('GoogleSignInPort', () {
    const offline = GoogleSignInException(
      code: GoogleSignInExceptionCode.unknownError,
      description: 'The Internet connection appears to be offline.',
    );

    test('client() signs in quietly again after an offline restore', () async {
      final gs = FakeGs()..lightweight.addAll([offline, FakeAccount('a@b.c')]);
      final p = GoogleSignInPort(gs);
      await expectLater(p.restore(), throwsA(offline));
      expect(await p.client(), isNotNull);
      expect(gs.attempts, 2);
      expect(p.currentEmail, 'a@b.c');
    });

    test('client() is null when no session is kept', () async {
      final gs = FakeGs()..lightweight.addAll([null, null]);
      final p = GoogleSignInPort(gs);
      expect(await p.restore(), isNull);
      expect(await p.client(), isNull);
    });

    test('an offline launch keeps the account after a later pass', () async {
      final gs = FakeGs()..lightweight.addAll([offline, FakeAccount('a@b.c')]);
      final a = GoogleAccount(GoogleSignInPort(gs));
      await a.restore();
      expect(a.currentEmail, isNull);
      expect(await a.driveStore(), isA<GoogleDriveStore>());
      expect(a.currentEmail, 'a@b.c');
    });

    test('client() turns a raw AppAuth network error into a retry', () async {
      final gs = FakeGs()
        ..lightweight.add(
          PlatformException(code: 'org.openid.appauth.general: -5'),
        );
      final p = GoogleSignInPort(gs);
      await expectLater(
        p.client(),
        throwsA(
          isA<StorageError>().having(
            (x) => x.kind,
            'kind',
            StorageErrorKind.retryable,
          ),
        ),
      );
    });

    test('offline during client() is a retry, not a re-login', () async {
      final gs = FakeGs()..lightweight.addAll([offline, offline]);
      final a = GoogleAccount(GoogleSignInPort(gs));
      await a.restore();
      await expectLater(
        a.driveStore(),
        throwsA(
          isA<StorageError>()
              .having((x) => x.kind, 'kind', StorageErrorKind.retryable)
              .having((x) => x.message, 'message', networkMessage),
        ),
      );
    });
  });

  test('raw AppAuth and URL errors from iOS are retryable', () async {
    for (final code in [
      'org.openid.appauth.general: -5',
      'NSURLErrorDomain: -1009',
    ]) {
      port.clientError = PlatformException(code: code);
      await expectLater(
        account.driveStore(),
        throwsA(
          isA<StorageError>()
              .having((x) => x.kind, 'kind', StorageErrorKind.retryable)
              .having((x) => x.message, 'message', networkMessage),
        ),
        reason: code,
      );
    }
  });

  test('a rejected refresh token (invalid_grant) asks to sign in', () async {
    port.clientError = PlatformException(
      code: 'org.openid.appauth.oauth_token: -10',
    );
    await expectLater(
      account.driveStore(),
      throwsA(
        isA<StorageError>().having(
          (x) => x.kind,
          'kind',
          StorageErrorKind.authExpired,
        ),
      ),
    );
  });

  group('late restore', () {
    test('a restore finishing after a sign-in is ignored', () async {
      final hold = Completer<void>();
      port
        ..restoreHold = hold.future
        ..restored = null;
      final r = account.restore();
      expect(await account.signIn(), 'me@example.com');
      hold.complete();
      await r;
      expect(account.currentEmail, 'me@example.com');
    });

    test('a restore finishing after a sign-out is ignored', () async {
      final hold = Completer<void>();
      port
        ..restoreHold = hold.future
        ..restored = 'old@example.com';
      final r = account.restore();
      await account.signOut();
      hold.complete();
      await r;
      expect(account.currentEmail, isNull);
    });

    test('the port drops a restore that ends after a sign-out', () async {
      final gate = Completer<void>();
      final gs = FakeGs()
        ..lightweight.add(FakeAccount('old@b.c'))
        ..gate = gate.future;
      final p = GoogleSignInPort(gs);
      final r = p.restore();
      await pumpEventQueue();
      await p.signOut();
      gate.complete();
      await r;
      expect(p.currentEmail, isNull);
    });
  });

  test('network failures in client() are retryable', () async {
    for (final e in <Object>[
      const GoogleSignInException(code: GoogleSignInExceptionCode.unknownError),
      const SocketException('offline'),
      http.ClientException('offline'),
      TimeoutException('slow'),
    ]) {
      port.clientError = e;
      await expectLater(
        account.driveStore(),
        throwsA(
          isA<StorageError>()
              .having((x) => x.kind, 'kind', StorageErrorKind.retryable)
              .having((x) => x.message, 'message', networkMessage),
        ),
        reason: '$e',
      );
    }
  });

  test('a user mismatch still asks for a new sign-in', () async {
    port.clientError = const GoogleSignInException(
      code: GoogleSignInExceptionCode.userMismatch,
    );
    await expectLater(
      account.driveStore(),
      throwsA(
        isA<StorageError>().having(
          (x) => x.kind,
          'kind',
          StorageErrorKind.authExpired,
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

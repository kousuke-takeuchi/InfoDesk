import 'package:extension_google_sign_in_as_googleapis_auth/extension_google_sign_in_as_googleapis_auth.dart';
import 'package:google_sign_in/google_sign_in.dart';
import 'package:http/http.dart' as http;

import '../storage/drive_store.dart';
import '../storage/google_drive_store.dart';

const googleScopes = <String>[
  'https://www.googleapis.com/auth/drive.appdata',
  'openid',
  'email',
];

/// Runs [_run] once successfully. A failed run is forgotten, so the next call
/// tries again.
class LazyInit {
  LazyInit(this._run);

  final Future<void> Function() _run;
  Future<void>? _f;

  Future<void> ready() {
    final f = _f ??= _run();
    return f.catchError((Object e) {
      if (identical(_f, f)) _f = null;
      throw e;
    });
  }
}

/// The part of google_sign_in the app uses, so tests never reach native code.
abstract class GoogleAuthPort {
  /// A session left from an earlier launch, without any UI. The email, or null.
  Future<String?> restore();

  /// Interactive sign-in plus authorization of [googleScopes]. Returns the
  /// email. Throws [GoogleSignInException] (code `canceled` when dismissed).
  Future<String> signIn();

  /// An authenticated client when the authorization is still usable without
  /// UI, or null when the user has to sign in again.
  Future<http.Client?> client();

  Future<void> signOut();
}

/// The real thing, over google_sign_in 7 (`GoogleSignIn.instance`). On iOS the
/// client id comes from `GIDClientID` in Info.plist.
class GoogleSignInPort implements GoogleAuthPort {
  GoogleSignInPort([GoogleSignIn? signIn])
    : _gs = signIn ?? GoogleSignIn.instance;

  final GoogleSignIn _gs;
  late final LazyInit _init = LazyInit(_gs.initialize);
  GoogleSignInAccount? _account;
  String? _token;

  Future<void> _ready() => _init.ready();

  @override
  Future<String?> restore() async {
    await _ready();
    final a = await _gs.attemptLightweightAuthentication();
    _account = a;
    return a?.email;
  }

  @override
  Future<String> signIn() async {
    await _ready();
    final stale = _token;
    if (stale != null) {
      // A token that was rejected must not be handed out again.
      await _gs.authorizationClient.clearAuthorizationToken(accessToken: stale);
      _token = null;
    }
    final a = await _gs.authenticate(scopeHint: googleScopes);
    final authz = await a.authorizationClient.authorizeScopes(googleScopes);
    _account = a;
    _token = authz.accessToken;
    return a.email;
  }

  @override
  Future<http.Client?> client() async {
    final a = _account;
    if (a == null) return null;
    final authz = await a.authorizationClient.authorizationForScopes(
      googleScopes,
    );
    if (authz == null) return null;
    _token = authz.accessToken;
    return authz.authClient(scopes: googleScopes);
  }

  @override
  Future<void> signOut() async {
    await _ready();
    await _gs.signOut();
    _account = null;
    _token = null;
  }
}

class GoogleAccount {
  GoogleAccount(this._port);

  final GoogleAuthPort _port;
  String? _email;

  String? get currentEmail => _email;

  Future<void> restore() async {
    try {
      _email = await _port.restore();
    } on Object {
      // Nothing to show or log: the user just signs in again.
      _email = null;
    }
  }

  /// The email, or null when the user dismissed the sign-in.
  Future<String?> signIn() async {
    try {
      return _email = await _port.signIn();
    } on GoogleSignInException catch (e) {
      if (e.code == GoogleSignInExceptionCode.canceled) return null;
      throw const StorageError(StorageErrorKind.other, 'Google にログインできませんでした');
    }
  }

  Future<void> signOut() async {
    await _port.signOut();
    _email = null;
  }

  /// Throws an [StorageErrorKind.authExpired] error when the user has to sign
  /// in again.
  Future<GoogleDriveStore> driveStore() async {
    try {
      final c = await _port.client();
      if (c != null) return GoogleDriveStore.withClient(c);
    } on StorageError {
      rethrow;
    } on Object {
      // A GoogleSignInException or anything else raw: sign in again.
    }
    throw const StorageError(StorageErrorKind.authExpired, reauthMessage);
  }
}

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../data/local_store.dart';
import '../storage/nas_store.dart' show nasHttpsMessage, nasUrlMessage;

enum Backend { gdrive, nas }

/// Where the NAS password lives. The real one is the keychain/keystore.
abstract class SecretStore {
  Future<String?> read(String key);
  Future<void> write(String key, String value);
  Future<void> delete(String key);
}

class FlutterSecretStore implements SecretStore {
  FlutterSecretStore([FlutterSecureStorage? storage])
    : _s = storage ?? const FlutterSecureStorage();
  final FlutterSecureStorage _s;

  @override
  Future<String?> read(String key) => _s.read(key: key);
  @override
  Future<void> write(String key, String value) =>
      _s.write(key: key, value: value);
  @override
  Future<void> delete(String key) => _s.delete(key: key);
}

class MemorySecretStore implements SecretStore {
  final values = <String, String>{};
  @override
  Future<String?> read(String key) async => values[key];
  @override
  Future<void> write(String key, String value) async => values[key] = value;
  @override
  Future<void> delete(String key) async => values.remove(key);
}

/// A setting the user typed that cannot be saved. [message] is Japanese.
class InvalidSetting implements Exception {
  const InvalidSetting(this.message);
  final String message;
  @override
  String toString() => message;
}

/// Same rule as the Mac: http only, a host, and no user info. Returns the
/// message to show, or null when the URL is usable.
String? validateNasUrl(String raw) {
  final Uri u;
  try {
    u = Uri.parse(raw.trim());
  } on FormatException {
    return nasUrlMessage;
  }
  if (u.scheme == 'https') return nasHttpsMessage;
  if (u.scheme != 'http' || u.host.isEmpty || u.userInfo.isNotEmpty) {
    return nasUrlMessage;
  }
  return null;
}

class NasConfig {
  const NasConfig({required this.url, required this.user, this.password});
  final String url;
  final String user;
  final String? password;

  bool get complete =>
      url.isNotEmpty && user.isNotEmpty && (password ?? '').isNotEmpty;
}

const _kBackend = 'backend';
const _kNasUrl = 'nas.url';
const _kNasUser = 'nas.user';
const _kNasPassword = 'nas.password';

/// The backend choice and the NAS connection. The URL and user are in the
/// local database; the password only in the [SecretStore].
class AppSettings {
  AppSettings(this._store, this._secrets);

  final LocalStore _store;
  final SecretStore _secrets;

  /// Anything but `nas` (also a stale `google`) reads as Google Drive.
  Future<Backend> backend() async => await _store.getSetting(_kBackend) == 'nas'
      ? Backend.nas
      : Backend.gdrive;

  /// Switching wipes the local copy and the synced-file records, so the next
  /// pass imports everything from the new storage. Returns whether it changed.
  Future<bool> setBackend(Backend b) async {
    if (await backend() == b) return false;
    await _store.clearLocalCopy();
    await _store.setSetting(_kBackend, b.name);
    return true;
  }

  Future<NasConfig> nas() async => NasConfig(
    url: await _store.getSetting(_kNasUrl) ?? '',
    user: await _store.getSetting(_kNasUser) ?? '',
    password: await _secrets.read(_kNasPassword),
  );

  /// An empty [url] is allowed (not entered yet). A null [password] keeps the
  /// saved one; an empty one removes it. While NAS is the backend, a changed
  /// URL wipes the local copy, as on the Mac.
  Future<void> setNas({
    required String url,
    required String user,
    String? password,
  }) async {
    final trimmed = url.trim();
    if (trimmed.isNotEmpty) {
      final bad = validateNasUrl(trimmed);
      if (bad != null) throw InvalidSetting(bad);
    }
    final old = await _store.getSetting(_kNasUrl) ?? '';
    if (trimmed != old && await backend() == Backend.nas) {
      await _store.clearLocalCopy();
    }
    await _store.setSetting(_kNasUrl, trimmed);
    await _store.setSetting(_kNasUser, user.trim());
    if (password != null) {
      if (password.isEmpty) {
        await _secrets.delete(_kNasPassword);
      } else {
        await _secrets.write(_kNasPassword, password);
      }
    }
  }

  /// Removes the URL, user and password. The backend choice stays.
  Future<void> clearNas() async {
    await _store.setSetting(_kNasUrl, '');
    await _store.setSetting(_kNasUser, '');
    await _secrets.delete(_kNasPassword);
  }
}

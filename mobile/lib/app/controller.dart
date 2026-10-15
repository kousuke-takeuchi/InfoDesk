import 'dart:async';

import 'package:flutter/foundation.dart';

import '../auth/google_account.dart';
import '../auth/tailnet_service.dart';
import '../data/local_store.dart';
import '../settings/app_settings.dart';
import '../storage/drive_store.dart';
import '../storage/storage_factory.dart';
import '../sync/engine.dart';
import 'errors.dart';
import 'version.dart';

const _kOnboarded = 'onboarded';
const _kLastSync = 'last_sync_at';
const _kWrittenAt = 'manifest.written_at';
const _kWriter = 'manifest.writer';

/// What the screens read and call. One instance for the whole app, holding
/// the one [SyncEngine]. Storage changes and sync passes never overlap.
class AppController extends ChangeNotifier {
  AppController({
    required this.store,
    required this.engine,
    required this.settings,
    required this.storage,
    required this.google,
    required this.tailnet,
    required this.openArticle,
    required this.openExternal,
    DateTime Function()? clock,
    this.version = appVersion,
  }) : now = clock ?? DateTime.now;

  final LocalStore store;
  final SyncEngine engine;
  final AppSettings settings;
  final StorageFactory storage;
  final GoogleAccount google;
  final TailnetService tailnet;

  /// Opens an article in the in-app browser (SFSafariViewController on iOS).
  final UrlOpener openArticle;

  /// Opens a page in the system browser.
  final UrlOpener openExternal;
  final DateTime Function() now;
  final String version;

  bool ready = false;
  bool onboarded = false;
  Backend backend = Backend.gdrive;
  bool syncing = false;
  String? syncError;
  bool authExpired = false;
  List<String> warnings = const [];
  DateTime? lastSyncAt;
  DateTime? macWrittenAt;
  String? macWriter;

  /// Error of the last settings action, in Japanese.
  String? actionError;
  String? nasCheckText;
  bool nasCheckPassed = false;
  bool nasChecking = false;
  NasConfig nasConfig = const NasConfig(url: '', user: '');

  /// Bumped when the stored data may have changed, so screens reload.
  int revision = 0;

  Future<void>? _syncing;
  Future<void> _tail = Future.value();

  String? get googleEmail => google.currentEmail;

  /// Loads the saved state. Does not sync.
  Future<void> init() async {
    try {
      await google.restore();
      backend = await settings.backend();
      onboarded = await store.getSetting(_kOnboarded) == 'true';
      await _loadSyncInfo();
      await _loadNas();
      // Tailscale may take long to come up; the first frame never waits.
      unawaited(
        tailnet
            .startOnLaunch(nas: backend == Backend.nas)
            .then((_) {}, onError: (_) {}),
      );
    } finally {
      ready = true;
      notifyListeners();
    }
  }

  /// App open: load, then a first pass. Errors are shown, never thrown.
  Future<void> start() async {
    try {
      await init();
      if (onboarded) unawaited(sync());
    } catch (e) {
      actionError = describeError(e);
      notifyListeners();
    }
  }

  Future<void> _loadSyncInfo() async {
    lastSyncAt = DateTime.tryParse(
      await store.getSetting(_kLastSync) ?? '',
    )?.toLocal();
    macWrittenAt = DateTime.tryParse(
      await store.getSetting(_kWrittenAt) ?? '',
    )?.toLocal();
    macWriter = await store.getSetting(_kWriter);
  }

  Future<void> _loadNas() async {
    try {
      nasConfig = await settings.nas();
    } on Object {
      nasConfig = NasConfig(
        url: await store.getSetting('nas.url') ?? '',
        user: await store.getSetting('nas.user') ?? '',
      );
    }
  }

  // Serialising: one storage-touching job at a time, in call order.
  Future<T> _exclusive<T>(Future<T> Function() job) {
    final done = Completer<T>();
    final prev = _tail;
    _tail = done.future.then((_) {}, onError: (_) {});
    prev.then((_) async {
      try {
        done.complete(await job());
      } catch (e, st) {
        done.completeError(e, st);
      }
    });
    return done.future;
  }

  /// A call during a pass returns that pass.
  Future<void> sync() {
    final cur = _syncing;
    if (cur != null) return cur;
    late final Future<void> f;
    f = _exclusive(_pass).whenComplete(() {
      if (identical(_syncing, f)) _syncing = null;
    });
    return _syncing = f;
  }

  Future<void> _pass() async {
    syncing = true;
    notifyListeners();
    try {
      final r = await engine.runOnce();
      syncError = null;
      authExpired = false;
      warnings = r.warnings;
      lastSyncAt = now();
      await store.setSetting(_kLastSync, lastSyncAt!.toUtc().toIso8601String());
      await _loadSyncInfo();
      revision++;
    } on StorageError catch (e) {
      syncError = e.message;
      authExpired = e.kind == StorageErrorKind.authExpired;
    } catch (e) {
      syncError = describeError(e);
    } finally {
      syncing = false;
      notifyListeners();
    }
  }

  Future<void> _markOnboarded() async {
    onboarded = true;
    await store.setSetting(_kOnboarded, 'true');
  }

  // ---- settings actions ----

  Future<void> _action(Future<void> Function() job) async {
    actionError = null;
    try {
      await job();
    } catch (e) {
      actionError = describeError(e);
    }
    notifyListeners();
  }

  /// Waits for a running pass, then switches the storage.
  Future<void> setBackend(Backend b) => _action(() async {
    final changed = await _exclusive(() => settings.setBackend(b));
    backend = b;
    nasCheckText = null;
    if (changed) {
      syncError = null;
      authExpired = false;
      revision++;
      await _loadNas();
    }
    notifyListeners();
    if (b == Backend.nas) {
      await tailnet.startOnLaunch(nas: true);
    } else if (changed && googleEmail != null) {
      unawaited(sync());
    }
  });

  Future<void> signInGoogle() => _action(() async {
    final email = await google.signIn();
    if (email == null) return;
    authExpired = false;
    syncError = null;
    await _markOnboarded();
    unawaited(sync());
  });

  Future<void> signOutGoogle() => _action(() async {
    await google.signOut();
  });

  Future<void> saveNas({
    required String url,
    required String user,
    String? password,
  }) => _action(() async {
    await _exclusive(
      () => settings.setNas(url: url, user: user, password: password),
    );
    nasCheckText = null;
    await _loadNas();
    revision++;
  });

  Future<void> clearNas() => _action(() async {
    await _exclusive(settings.clearNas);
    nasCheckText = null;
    await _loadNas();
  });

  /// "確かめる": lists the folder. A pass has priority over the check.
  Future<void> checkNas() async {
    nasChecking = true;
    nasCheckText = null;
    notifyListeners();
    try {
      final text = await _exclusive(storage.checkNas);
      nasCheckText = text;
      nasCheckPassed = text == nasCheckOk;
      if (nasCheckPassed && !onboarded) {
        await _markOnboarded();
        unawaited(sync());
      }
    } catch (e) {
      nasCheckText = describeError(e);
      nasCheckPassed = false;
    } finally {
      nasChecking = false;
      notifyListeners();
    }
  }

  /// Deletes the local copy; the next pass imports everything again.
  Future<void> clearLocalCopy() => _action(() async {
    await _exclusive(store.clearLocalCopy);
    lastSyncAt = null;
    macWrittenAt = null;
    macWriter = null;
    await store.setSetting(_kLastSync, '');
    revision++;
    notifyListeners();
    unawaited(sync());
  });

  // ---- tailscale ----

  Future<void> tailnetLogin() => _action(tailnet.login);
  Future<void> tailnetLogout() => _action(tailnet.logout);
  Future<void> tailnetForget() => _action(tailnet.forget);
  Future<void> tailnetRetry() => _action(() async {
    await tailnet.stop();
    unawaited(tailnet.start());
  });

  // ---- reading ----

  Future<String> _device() => store.deviceId();

  /// Local writes wait for a running pass, and report failures in
  /// [actionError] instead of throwing into a button handler.
  Future<void> _write(Future<void> Function() job) async {
    try {
      await _exclusive(job);
      actionError = null;
      revision++;
    } catch (e) {
      actionError = describeError(e);
    }
    notifyListeners();
  }

  Future<void> setRead(int id, bool value) => _write(() async {
    await store.setRead(id, value, await _device());
  });

  Future<void> setStarred(int id, bool value) => _write(() async {
    await store.setStarred(id, value, await _device());
  });

  Future<int> markAllRead(ItemFilter filter) async {
    var n = 0;
    await _write(() async {
      n = await store.markAllRead(filter, await _device());
    });
    return n;
  }

  /// Opens the article in the in-app browser and marks it read. False when
  /// the page could not be opened.
  Future<bool> openItem(StoredItem item) async {
    if (!item.isRead) await setRead(item.record.id, true);
    final uri = Uri.tryParse(item.record.url);
    if (uri == null || (uri.scheme != 'https' && uri.scheme != 'http')) {
      return false;
    }
    try {
      return await openArticle(uri);
    } on Object {
      return false;
    }
  }
}

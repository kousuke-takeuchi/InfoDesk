import 'dart:convert';

import 'package:crypto/crypto.dart';

import '../data/local_store.dart';
import '../storage/drive_store.dart';
import 'format.dart';

/// What one pass did.
class SyncReport {
  SyncReport({
    this.downloaded = 0,
    this.appliedOps = 0,
    this.uploadedOps = 0,
    List<String>? warnings,
  }) : warnings = warnings ?? [];

  /// Meta, day, digest and snapshot files fetched and imported.
  int downloaded;
  int appliedOps;
  int uploadedOps;

  /// Problems that did not stop the pass. Japanese, fit for the screen.
  final List<String> warnings;

  /// True when the UI should refresh.
  bool get changed => downloaded > 0 || appliedOps > 0;
}

/// One sync pass on the phone: reads what the Mac published, applies other
/// devices' ops, and writes this device's own ops. Nothing is recorded as
/// done until it has been applied, so a failed pass is simply run again.
/// Thrown by a pass that [SyncEngine.cancel] stopped. Not an error: what the
/// pass imported stays recorded and the next pass picks up the rest.
class SyncCancelled implements Exception {
  const SyncCancelled();
}

class SyncEngine {
  SyncEngine(LocalStore store, DriveStore drive)
    : this.lazy(store, () async => drive);

  /// [driveFor] is called at the start of every pass, so each pass gets a
  /// fresh store (its Google client can expire between passes).
  SyncEngine.lazy(this._store, this._driveFor);

  final LocalStore _store;
  final Future<DriveStore> Function() _driveFor;
  late DriveStore _drive;
  Future<SyncReport>? _running;
  bool _cancel = false;

  /// Stops the running pass at its next step (a storage switch must not wait
  /// for a long first sync). Does nothing when no pass runs.
  void cancel() {
    if (_running != null) _cancel = true;
  }

  void _check() {
    if (_cancel) throw const SyncCancelled();
  }

  /// A call while a pass is running returns that pass's future.
  Future<SyncReport> runOnce({DateTime? now}) {
    final cur = _running;
    if (cur != null) return cur;
    _cancel = false;
    final f = _pass(now ?? DateTime.now()).whenComplete(() => _running = null);
    _running = f;
    return f;
  }

  Future<SyncReport> _pass(DateTime now) async {
    final report = SyncReport();
    _drive = await _driveFor();
    _check();
    final byName = <String, RemoteFile>{};
    for (final f in await _drive.list()) {
      if (parseFileName(f.name) != null) byName[f.name] = f;
    }

    final mf = byName['manifest.json'];
    if (mf == null) {
      report.warnings.add('同期フォルダにまだデータがありません。Mac で同期してください。');
      return report;
    }
    final Manifest manifest;
    try {
      manifest = Manifest.fromJson(
        jsonDecode(utf8.decode(await _drive.download(mf)))
            as Map<String, dynamic>,
      );
    } on StorageError {
      rethrow;
    } catch (e) {
      report.warnings.add('manifest.json を読めませんでした');
      return report;
    }

    // A newer format may mean anything; touch nothing until the app knows it.
    if (manifest.schema != 1) {
      report.warnings.add('同期データの形式が新しくなっています。アプリを更新してください');
      return report;
    }

    await _store.setSetting('manifest.written_at', manifest.writtenAt);
    await _store.setSetting('manifest.writer', manifest.writer);
    if (manifest.dbId != await _store.dbId()) {
      await _store.resetForDbId(manifest.dbId);
    }

    // Data files, in the order the Mac's own import uses: meta, days,
    // digests, then the snapshot on top.
    final data = [
      for (final e in manifest.files.entries)
        if (byName.containsKey(e.key) &&
            const {
              FileKindType.meta,
              FileKindType.day,
              FileKindType.digest,
              FileKindType.snapshot,
            }.contains(parseFileName(e.key)!.kind))
          e,
    ]..sort((a, b) => _rank(a.key).compareTo(_rank(b.key)));

    for (final e in data) {
      final name = e.key;
      final kind = parseFileName(name)!.kind;
      final rec = await _store.syncedFile(name);
      if (rec != null && rec.imported && rec.sha256 == e.value.sha256) continue;
      _check();
      final bytes = await _drive.download(byName[name]!);
      _check();
      if (sha256.convert(bytes).toString() != e.value.sha256) {
        report.warnings.add('$name のハッシュが一致しないため取り込みませんでした');
        continue;
      }
      // Only decode and parse failures are warnings; store errors propagate.
      final Object parsed;
      try {
        final json = gunzipJson(bytes);
        switch (kind) {
          case FileKindType.meta:
            parsed = MetaFile.fromJson(json);
          case FileKindType.day:
            parsed = DayFile.fromJson(json);
          case FileKindType.digest:
            parsed = DigestFile.fromJson(json);
          case FileKindType.snapshot:
            parsed = StateSnapshot.fromJson(json);
          default:
            continue;
        }
      } catch (e) {
        report.warnings.add('$name を読めませんでした');
        continue;
      }
      switch (parsed) {
        case MetaFile():
          await _store.putMeta(parsed);
        case DayFile():
          await _store.upsertDay(parsed);
        case DigestFile():
          await _store.upsertDigests(parsed);
        case StateSnapshot():
          await _store.applySnapshot(parsed);
      }
      await _store.putSyncedFile(name, e.value.sha256, kind.name);
      report.downloaded++;
    }

    // Every ops file, this device's own included (after a local wipe they
    // may be the only copy of its marks), oldest first.
    final me = await _store.deviceId();
    final others =
        [
          for (final f in byName.values)
            if (parseFileName(f.name)!.kind == FileKindType.ops) f,
        ]..sort((a, b) {
          final c = _opsOrder(a.name).compareTo(_opsOrder(b.name));
          return c != 0 ? c : a.name.compareTo(b.name);
        });
    for (final f in others) {
      final rec = await _store.syncedFile(f.name);
      if (rec != null && rec.imported) continue;
      _check();
      final bytes = await _drive.download(f);
      _check();
      final sha = sha256.convert(bytes).toString();
      List<Op>? ops;
      try {
        final file = OpsFile.fromJson(
          jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>,
        );
        // Written against another database: its item ids name other
        // articles. Recorded below, never applied.
        final other = file.dbId != null && file.dbId != manifest.dbId;
        ops = other ? null : file.ops;
      } catch (_) {
        report.warnings.add('${f.name} を読めませんでした');
      }
      if (ops != null) {
        await _store.applyRemoteOps(ops);
        report.appliedOps += ops.length;
      }
      await _store.putSyncedFile(f.name, sha, 'ops');
    }

    // This device's own ops, last. Marked sent only after the create.
    _check();
    final pending = await _store.pendingOps();
    if (pending.isNotEmpty) {
      final mine = [for (final p in pending) p.op];
      // A storage switch wipes item_state but keeps these; put them back so
      // the marks stay visible (idempotent when they are already there).
      await _store.applyRemoteOps(mine);
      final file = OpsFile(device: me, dbId: manifest.dbId, ops: mine);
      final name = opsFileName(me, now, 1);
      final body = utf8.encode(jsonEncode(file.toJson()));
      await _drive.create(name, body, 'application/json');
      // Already applied here, so this install never downloads it again.
      await _store.putSyncedFile(name, sha256.convert(body).toString(), 'ops');
      await _store.markOpsSent([for (final p in pending) p.id]);
      report.uploadedOps = pending.length;
    }
    return report;
  }

  static int _rank(String name) => switch (parseFileName(name)!.kind) {
    FileKindType.meta => 0,
    FileKindType.day => 1,
    FileKindType.digest => 2,
    _ => 3,
  };

  /// Sort key: the time stamp and sequence at the end of the name.
  static final _opsStamp = RegExp(r'-(\d{8}T\d{9}Z-\d{4})\.json$');
  static String _opsOrder(String name) {
    final m = _opsStamp.firstMatch(name);
    return m?[1] ?? name;
  }
}

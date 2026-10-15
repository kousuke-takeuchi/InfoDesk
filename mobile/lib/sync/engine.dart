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
class SyncEngine {
  SyncEngine(this._store, this._drive);

  final LocalStore _store;
  final DriveStore _drive;
  Future<SyncReport>? _running;

  /// A call while a pass is running returns that pass's future.
  Future<SyncReport> runOnce({DateTime? now}) {
    final cur = _running;
    if (cur != null) return cur;
    final f = _pass(now ?? DateTime.now()).whenComplete(() => _running = null);
    _running = f;
    return f;
  }

  Future<SyncReport> _pass(DateTime now) async {
    final report = SyncReport();
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
      final bytes = await _drive.download(byName[name]!);
      if (sha256.convert(bytes).toString() != e.value.sha256) {
        report.warnings.add('$name のハッシュが一致しないため取り込みませんでした');
        continue;
      }
      try {
        final json = gunzipJson(bytes);
        switch (kind) {
          case FileKindType.meta:
            await _store.putMeta(MetaFile.fromJson(json));
          case FileKindType.day:
            await _store.upsertDay(DayFile.fromJson(json));
          case FileKindType.digest:
            await _store.upsertDigests(DigestFile.fromJson(json));
          case FileKindType.snapshot:
            await _store.applySnapshot(StateSnapshot.fromJson(json));
          default:
            continue;
        }
      } catch (e) {
        report.warnings.add('$name を読めませんでした');
        continue;
      }
      await _store.putSyncedFile(name, e.value.sha256, kind.name);
      report.downloaded++;
    }

    // Other devices' ops, oldest first.
    final me = await _store.deviceId();
    final others = [
      for (final f in byName.values)
        if (parseFileName(f.name)!.kind == FileKindType.ops &&
            parseFileName(f.name)!.device != me)
          f,
    ]..sort((a, b) => _opsOrder(a.name).compareTo(_opsOrder(b.name)));
    for (final f in others) {
      final rec = await _store.syncedFile(f.name);
      if (rec != null && rec.imported) continue;
      final bytes = await _drive.download(f);
      final sha = sha256.convert(bytes).toString();
      List<Op>? ops;
      try {
        ops = OpsFile.fromJson(
          jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>,
        ).ops;
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
    final pending = await _store.pendingOps();
    if (pending.isNotEmpty) {
      final file = OpsFile(device: me, ops: [for (final p in pending) p.op]);
      await _drive.create(
        opsFileName(me, now, 1),
        utf8.encode(jsonEncode(file.toJson())),
        'application/json',
      );
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
  static String _opsOrder(String name) {
    final m = RegExp(r'-(\d{8}T\d{9}Z-\d{4})\.json$').firstMatch(name);
    return m?[1] ?? name;
  }
}

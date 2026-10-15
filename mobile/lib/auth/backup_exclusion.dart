import 'dart:io';

import 'package:flutter/services.dart';

const backupChannelName = 'jp.infodesk.mobile/backup';

/// Keeps a directory out of the iOS backup (`NSURLIsExcludedFromBackupKey`),
/// as the tailscale package requires for its state directory
/// (doc/adr-encrypted-node-state.md, "Backup-exclusion ownership"). Dart has no
/// API for it, so one small method channel in `AppDelegate.swift` sets the key
/// and reads it back. The state directory stays in Application Support, which
/// is never purged by the system, unlike Caches. Other platforms: no-op.
class BackupExclusion {
  BackupExclusion({MethodChannel? channel, bool? isIOS})
    : _channel = channel ?? const MethodChannel(backupChannelName),
      _ios = isIOS ?? Platform.isIOS;

  final MethodChannel _channel;
  final bool _ios;

  /// Throws when the exclusion could not be set and verified.
  Future<void> exclude(String path) async {
    if (!_ios) return;
    final ok = await _channel.invokeMethod<bool>('exclude', path);
    if (ok != true) {
      throw StateError('backup exclusion was not applied');
    }
  }
}

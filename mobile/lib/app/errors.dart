import '../auth/tailnet_service.dart' show TailnetInitError;
import '../settings/app_settings.dart' show InvalidSetting;
import '../storage/drive_store.dart' show StorageError;

const genericErrorMessage = '処理に失敗しました。もう一度お試しください';

/// Japanese text for anything the UI may catch. Raw exception text never
/// reaches the screen.
String describeError(Object e) => switch (e) {
  StorageError(:final message) => message,
  TailnetInitError(:final message) => message,
  InvalidSetting(:final message) => message,
  _ => genericErrorMessage,
};

final _nonAscii = RegExp(r'[^\x00-\x7F]');

/// A Tailscale failure reason is shown only when it is already Japanese.
String safeReason(String reason) =>
    _nonAscii.hasMatch(reason) ? reason : 'Tailscale を起動できませんでした';

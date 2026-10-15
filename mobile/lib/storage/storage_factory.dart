import '../auth/google_account.dart';
import '../auth/tailnet_service.dart';
import '../settings/app_settings.dart';
import 'drive_store.dart';
import 'nas_store.dart';
import 'tailnet_port.dart';

const nasIncompleteMessage = 'NAS の URL、ユーザー名、パスワードを入力してください';
const nasCheckOk = '読み取れました';

/// Builds the [DriveStore] for the chosen backend. Failures are
/// [StorageError]s with Japanese text.
class StorageFactory {
  StorageFactory({
    required this.settings,
    required this.google,
    required this.tailnet,
    TailnetPort Function()? nasPort,
  }) : _nasPort = nasPort ?? tailnet.port;

  final AppSettings settings;
  final GoogleAccount google;
  final TailnetService tailnet;
  final TailnetPort Function() _nasPort;

  Future<DriveStore> build() async => switch (await settings.backend()) {
    Backend.gdrive => await google.driveStore(),
    Backend.nas => await buildNas(),
  };

  /// Needs Tailscale Running and a complete URL, user and password.
  Future<NasStore> buildNas() async {
    switch (tailnet.state.value) {
      case TailnetRunning():
        break;
      case TailnetNeedsApproval():
        throw const StorageError(
          StorageErrorKind.other,
          tailnetApprovalMessage,
        );
      case TailnetStopped() when await tailnet.hasSavedState():
        throw const StorageError(StorageErrorKind.other, tailnetStoppedMessage);
      default:
        throw const StorageError(StorageErrorKind.other, tailnetLoginMessage);
    }
    final nas = await settings.nas();
    if (!nas.complete) {
      throw const StorageError(StorageErrorKind.other, nasIncompleteMessage);
    }
    return NasStore(
      tailnet: _nasPort(),
      url: nas.url,
      user: nas.user,
      password: nas.password!,
    );
  }

  /// "Check": lists the folder and nothing else (the phone writes no temp
  /// file). Returns the text to show.
  Future<String> checkNas() async {
    try {
      await (await buildNas()).list();
      return nasCheckOk;
    } on StorageError catch (e) {
      return e.message;
    }
  }
}

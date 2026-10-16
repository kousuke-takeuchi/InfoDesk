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
/// The NAS cannot be reached because Tailscale is not Running. A pass that
/// fails with it is run again once Tailscale comes up.
class TailnetNotReady extends StorageError {
  const TailnetNotReady(String message)
    : super(StorageErrorKind.other, message);
}

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

  /// Needs Tailscale Running and a complete URL, user and password. Never
  /// waits: it runs under the storage lock. The controller waits for a
  /// Tailscale start before taking the lock. Tailscale failures are
  /// [TailnetNotReady].
  Future<NasStore> buildNas() async {
    switch (tailnet.state.value) {
      case TailnetRunning():
        break;
      case TailnetNeedsApproval():
        throw const TailnetNotReady(tailnetApprovalMessage);
      case TailnetStopped() when await tailnet.hasSavedState():
        throw const TailnetNotReady(tailnetStoppedMessage);
      default:
        throw const TailnetNotReady(nasTailscaleLoginMessage);
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

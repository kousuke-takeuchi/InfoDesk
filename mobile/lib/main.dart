import 'package:flutter/material.dart';
import 'package:tailscale/tailscale.dart' show NodeState, TailscaleStatus;
import 'package:path_provider/path_provider.dart';
import 'package:url_launcher/url_launcher.dart';

import 'app/app.dart';
import 'app/controller.dart';
import 'auth/google_account.dart';
import 'auth/tailnet_service.dart';
import 'data/local_store.dart';
import 'settings/app_settings.dart';
import 'storage/storage_factory.dart';
import 'sync/engine.dart';

/// Stands in for the real node when Tailscale cannot be set up. Every call
/// fails with the Japanese reason, which the settings screen shows.
class _UnavailableNode implements TailnetNode {
  _UnavailableNode(this.error);
  final TailnetInitError error;

  @override
  Stream<NodeState> get onStateChange => const Stream.empty();
  @override
  Future<TailscaleStatus> up(String hostname) => Future.error(error);
  @override
  Future<TailscaleStatus> status() => Future.error(error);
  @override
  Future<void> logout() => Future.error(error);
  @override
  Future<void> forgetLocalIdentity() => Future.error(error);
  @override
  Future<void> down() => Future.error(error);
  @override
  Future<bool> hasSavedState() async => false;
}

Future<AppController> buildController() async {
  final dir = await getApplicationSupportDirectory();
  final store = await LocalStore.open('${dir.path}/infodesk.db');
  final settings = AppSettings(store, FlutterSecretStore());
  final google = GoogleAccount(GoogleSignInPort());

  TailnetNode node;
  try {
    node = await RealTailnetNode.create();
  } on TailnetInitError catch (e) {
    node = _UnavailableNode(e);
  } on Object {
    node = _UnavailableNode(const TailnetInitError(tailnetInitMessage));
  }
  final tailnet = TailnetService(
    node,
    openUrl: (u) => launchUrl(u, mode: LaunchMode.externalApplication),
    portBuilder: node is RealTailnetNode ? node.port : null,
  );
  final storage = StorageFactory(
    settings: settings,
    google: google,
    tailnet: tailnet,
  );
  return AppController(
    store: store,
    engine: SyncEngine.lazy(store, storage.build),
    settings: settings,
    storage: storage,
    google: google,
    tailnet: tailnet,
    openArticle: (u) => launchUrl(u, mode: LaunchMode.inAppBrowserView),
    openExternal: (u) => launchUrl(u, mode: LaunchMode.externalApplication),
  );
}

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final controller = await buildController();
  runApp(InfoDeskApp(controller: controller));
  // The saved state loads after the first frame; a pass follows.
  controller.start();
}

import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';
import 'package:tailscale/tailscale.dart';

import '../storage/tailnet_port.dart';

const tailnetHostname = 'infodesk-mobile';
const phaseConnecting = 'Tailscale のサーバーにつないでいます';
const phasePreparingLogin = 'ログインの準備をしています';
const tailnetLoginMessage = 'Tailscale にログインしてください';
const tailnetStoppedMessage = 'Tailscale が停止中です';
const tailnetApprovalMessage = 'Tailscale の管理画面でこの端末を承認してください';

sealed class TailnetState {
  const TailnetState();
}

class TailnetStopped extends TailnetState {
  const TailnetStopped();
}

class TailnetStarting extends TailnetState {
  const TailnetStarting(this.phase, this.startedAt);
  final String phase;

  /// When this start began, for the elapsed seconds on screen.
  final DateTime startedAt;
}

class TailnetNeedsLogin extends TailnetState {
  const TailnetNeedsLogin(this.url);
  final Uri url;
}

class TailnetNeedsApproval extends TailnetState {
  const TailnetNeedsApproval();
}

class TailnetRunning extends TailnetState {
  const TailnetRunning(this.name, this.ip);
  final String name;
  final String? ip;
}

class TailnetFailed extends TailnetState {
  const TailnetFailed(this.reason);
  final String reason;
}

/// The one-line status the settings screen shows.
String tailnetText(TailnetState s, DateTime now) => switch (s) {
  TailnetRunning(:final name) => '接続済み · $name',
  TailnetNeedsLogin() => 'ログインしてください',
  TailnetNeedsApproval() => tailnetApprovalMessage,
  TailnetStarting(:final phase, :final startedAt) =>
    '$phase (${now.difference(startedAt).inSeconds.clamp(0, 1 << 30)} 秒)',
  TailnetFailed(:final reason) => '起動できませんでした: $reason',
  TailnetStopped() => '停止中',
};

/// The part of the tailscale package the service uses. Tests fake it.
abstract class TailnetNode {
  Stream<NodeState> get onStateChange;

  /// Resolves on the first stable state. Never opens a browser.
  Future<TailscaleStatus> up(String hostname);
  Future<TailscaleStatus> status();
  Future<void> logout();
  Future<void> forgetLocalIdentity();
  Future<void> down();

  /// True when an earlier run left a Tailscale identity on this device.
  Future<bool> hasSavedState();
}

bool _initialized = false;

/// The real node over `Tailscale.instance`. State is kept under the app
/// support directory. Log upload to Tailscale is switched off.
class RealTailnetNode implements TailnetNode {
  RealTailnetNode._(this._ts, this._stateDir);

  static const appId = 'jp.infodesk.mobile';

  final Tailscale _ts;
  final String _stateDir;

  static Future<RealTailnetNode> create() async {
    final base = await getApplicationSupportDirectory();
    final dir = Directory('${base.path}/tailnet');
    await dir.create(recursive: true);
    if (!_initialized) {
      Tailscale.init(stateDir: dir.path, appId: appId, noLogsNoSupport: true);
      _initialized = true;
    }
    return RealTailnetNode._(Tailscale.instance, dir.path);
  }

  @override
  Stream<NodeState> get onStateChange => _ts.onStateChange;

  @override
  Future<TailscaleStatus> up(String hostname) => _ts.up(hostname: hostname);

  @override
  Future<TailscaleStatus> status() => _ts.status();

  @override
  Future<void> logout() => _ts.logout();

  @override
  Future<void> forgetLocalIdentity() => _ts.forgetLocalIdentity();

  @override
  Future<void> down() => _ts.down();

  @override
  Future<bool> hasSavedState() =>
      File('$_stateDir/tailscale/tailscaled.state.enc').exists();

  TailnetPort port() => TailscaleTailnetPort(_ts);
}

typedef UrlOpener = Future<bool> Function(Uri url);

class TailnetService {
  TailnetService(
    this._node, {
    required UrlOpener openUrl,
    DateTime Function()? clock,
    this.portBuilder,
  }) : _open = openUrl,
       _clock = clock ?? DateTime.now;

  final TailnetNode _node;
  final UrlOpener _open;
  final DateTime Function() _clock;
  final TailnetPort Function()? portBuilder;

  final state = ValueNotifier<TailnetState>(const TailnetStopped());

  StreamSubscription<NodeState>? _sub;
  Future<void>? _starting;
  bool _openWhenUrl = false;

  /// The tailnet-bound HTTP port for the NAS store.
  TailnetPort port() => (portBuilder ?? TailscaleTailnetPort.new)();

  Future<bool> hasSavedState() => _node.hasSavedState();

  /// On app launch: only for the NAS backend, and only when an earlier run
  /// left a Tailscale identity. Never opens the login URL.
  Future<void> startOnLaunch({required bool nas}) async {
    if (!nas || !await _node.hasSavedState()) return;
    await start();
  }

  /// Non-interactive. If a login is needed the state becomes NeedsLogin; the
  /// URL is not opened.
  Future<void> start() {
    final cur = state.value;
    if (cur is TailnetRunning || cur is TailnetNeedsApproval) {
      return Future.value();
    }
    return _starting ??= _start().whenComplete(() => _starting = null);
  }

  Future<void> _start() async {
    _sub ??= _node.onStateChange.listen((_) => unawaited(_refresh()));
    state.value = TailnetStarting(phaseConnecting, _clock());
    try {
      _apply(await _node.up(tailnetHostname));
    } on TailscaleException catch (e) {
      state.value = TailnetFailed(_reason(e));
    } on Exception catch (e) {
      state.value = TailnetFailed(e.toString());
    }
  }

  /// The user pressed "Tailscale にログイン": start if needed and open the
  /// login URL. When the URL is not known yet it opens as soon as it arrives.
  Future<void> login() async {
    _openWhenUrl = true;
    final cur = state.value;
    if (cur is TailnetNeedsLogin) {
      _openWhenUrl = false;
      await _open(cur.url);
      return;
    }
    await start();
  }

  Future<void> logout() async {
    _openWhenUrl = false;
    try {
      await _node.logout();
      state.value = const TailnetStopped();
    } on TailscaleException catch (e) {
      state.value = TailnetFailed(_reason(e));
    }
  }

  /// Drops the local identity without telling the control server. For when
  /// start keeps failing and logout cannot work either.
  Future<void> forget() async {
    _openWhenUrl = false;
    try {
      await _node.forgetLocalIdentity();
      state.value = const TailnetStopped();
    } on TailscaleException catch (e) {
      state.value = TailnetFailed(_reason(e));
    }
  }

  Future<void> stop() async {
    _openWhenUrl = false;
    await _node.down();
    state.value = const TailnetStopped();
  }

  Future<void> _refresh() async {
    try {
      _apply(await _node.status());
    } on TailscaleException catch (e) {
      state.value = TailnetFailed(_reason(e));
    }
  }

  void _apply(TailscaleStatus s) {
    switch (s.state) {
      case NodeState.running:
        _openWhenUrl = false;
        state.value = TailnetRunning(tailnetHostname, s.ipv4);
      case NodeState.needsMachineAuth:
        _openWhenUrl = false;
        state.value = const TailnetNeedsApproval();
      case NodeState.needsLogin:
        final url = s.authUrl;
        if (url == null) {
          _starting_(phasePreparingLogin);
        } else {
          state.value = TailnetNeedsLogin(url);
          if (_openWhenUrl) {
            _openWhenUrl = false;
            unawaited(_open(url));
          }
        }
      case NodeState.stopped:
        _openWhenUrl = false;
        state.value = const TailnetStopped();
      case NodeState.noState || NodeState.starting:
        _starting_(phaseConnecting);
      case NodeState.unknown:
        break;
    }
  }

  /// Keeps the start time while already starting.
  void _starting_(String phase) {
    final cur = state.value;
    final at = cur is TailnetStarting ? cur.startedAt : _clock();
    state.value = TailnetStarting(phase, at);
  }

  String _reason(TailscaleException e) => e.message;
}

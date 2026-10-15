import 'dart:async';

import 'package:flutter/material.dart';

import '../app/controller.dart';
import '../app/errors.dart';
import '../app/theme.dart';
import '../auth/tailnet_service.dart';

/// A start that takes longer than this gets the retry and reset buttons.
const slowStartSecs = 30;

/// The Tailscale state with its buttons. Same texts as the Mac's sync tab.
class TailnetSection extends StatefulWidget {
  const TailnetSection({super.key, required this.controller});
  final AppController controller;

  @override
  State<TailnetSection> createState() => _TailnetSectionState();
}

class _TailnetSectionState extends State<TailnetSection> {
  Timer? _timer;
  var _confirmForget = false;

  AppController get c => widget.controller;

  @override
  void initState() {
    super.initState();
    c.tailnet.state.addListener(_onState);
    _onState();
  }

  @override
  void dispose() {
    _timer?.cancel();
    c.tailnet.state.removeListener(_onState);
    super.dispose();
  }

  // One tick per second while starting, so the elapsed seconds move.
  void _onState() {
    final starting = c.tailnet.state.value is TailnetStarting;
    if (starting && _timer == null) {
      _timer = Timer.periodic(const Duration(seconds: 1), (_) {
        if (mounted) setState(() {});
      });
    } else if (!starting) {
      _timer?.cancel();
      _timer = null;
    }
    if (mounted) setState(() {});
  }

  String _text(TailnetState s) => switch (s) {
    TailnetFailed(:final reason) => '起動できませんでした: ${safeReason(reason)}',
    _ => tailnetText(s, c.now()),
  };

  @override
  Widget build(BuildContext context) {
    final s = c.tailnet.state.value;
    final running = s is TailnetRunning;
    final failed = s is TailnetFailed;
    final secs = s is TailnetStarting
        ? c.now().difference(s.startedAt).inSeconds
        : null;
    final slow = secs != null && secs >= slowStartSecs;
    final canForget = failed || slow;
    final confirm = _confirmForget && canForget;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            if (running)
              Container(
                width: 9,
                height: 9,
                margin: const EdgeInsets.only(right: 8),
                decoration: const BoxDecoration(
                  color: success,
                  shape: BoxShape.circle,
                ),
              ),
            Expanded(
              child: Text(_text(s), style: const TextStyle(fontSize: 16)),
            ),
            const SizedBox(width: 8),
            if (running)
              TextButton(onPressed: c.tailnetLogout, child: const Text('ログアウト'))
            else
              FilledButton(
                onPressed: c.tailnetLogin,
                style: FilledButton.styleFrom(backgroundColor: accent),
                child: Text(
                  s is TailnetNeedsLogin ? 'ログインページを開く' : 'Tailscale にログイン',
                ),
              ),
          ],
        ),
        const SizedBox(height: 4),
        const Text(
          'NAS と同じ Tailscale アカウントでログインします。InfoDesk の通信だけが通ります。',
          style: TextStyle(color: inkSecondary, fontSize: 13),
        ),
        if (slow) ...[
          const SizedBox(height: 12),
          const Text(
            '時間がかかっています',
            style: TextStyle(fontWeight: FontWeight.w600),
          ),
          Wrap(
            spacing: 8,
            children: [
              OutlinedButton(
                onPressed: c.tailnetRetry,
                child: const Text('もう一度試す'),
              ),
              if (!confirm)
                OutlinedButton(
                  onPressed: () => setState(() => _confirmForget = true),
                  child: const Text('状態を消してやり直す'),
                ),
            ],
          ),
        ],
        if (failed && !confirm)
          Align(
            alignment: Alignment.centerLeft,
            child: OutlinedButton(
              onPressed: () => setState(() => _confirmForget = true),
              child: const Text('この端末の Tailscale の状態を消す'),
            ),
          ),
        if (confirm) ...[
          const SizedBox(height: 8),
          const Text('Tailscale にもう一度ログインが必要になります。消しますか?'),
          Wrap(
            spacing: 8,
            children: [
              FilledButton(
                onPressed: () async {
                  await c.tailnetForget();
                  if (mounted) setState(() => _confirmForget = false);
                },
                style: FilledButton.styleFrom(backgroundColor: danger),
                child: const Text('消す'),
              ),
              OutlinedButton(
                onPressed: () => setState(() => _confirmForget = false),
                child: const Text('やめる'),
              ),
            ],
          ),
        ],
      ],
    );
  }
}

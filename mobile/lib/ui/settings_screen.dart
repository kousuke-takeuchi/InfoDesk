import 'package:flutter/rendering.dart' show ScrollCacheExtent;
import 'package:flutter/material.dart';

import '../app/controller.dart';
import '../app/dates.dart';
import '../app/theme.dart';
import '../settings/app_settings.dart';
import 'nas_form.dart';
import 'tailnet_section.dart';
import 'widgets.dart';

/// M-4.
class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key, required this.controller});
  final AppController controller;

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  ({int items, int digests})? _counts;
  var _confirmDelete = false;
  int _seen = -1;

  AppController get c => widget.controller;

  @override
  void initState() {
    super.initState();
    c.addListener(_onController);
    _loadCounts();
  }

  @override
  void dispose() {
    c.removeListener(_onController);
    super.dispose();
  }

  void _onController() {
    if (c.revision != _seen) _loadCounts();
    if (mounted) setState(() {});
  }

  Future<void> _loadCounts() async {
    _seen = c.revision;
    final counts = await c.store.localCounts();
    if (mounted) setState(() => _counts = counts);
  }

  Widget _header(String text) => Padding(
    padding: const EdgeInsets.fromLTRB(20, 24, 20, 8),
    child: Text(
      text,
      style: const TextStyle(
        color: inkSecondary,
        fontSize: 13,
        fontWeight: FontWeight.w600,
      ),
    ),
  );

  Widget _footer(String text) => Padding(
    padding: const EdgeInsets.fromLTRB(20, 8, 20, 0),
    child: Text(
      text,
      style: const TextStyle(color: inkSecondary, fontSize: 13, height: 1.45),
    ),
  );

  Widget _card(List<Widget> children) => Container(
    margin: const EdgeInsets.symmetric(horizontal: 16),
    padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
    decoration: BoxDecoration(
      color: surface,
      borderRadius: BorderRadius.circular(18),
    ),
    child: Column(children: children),
  );

  Widget _row(
    IconData icon,
    String title, {
    String? sub,
    Widget? trailing,
    Color? color,
  }) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 12),
    child: Row(
      children: [
        Icon(icon, color: color ?? inkSecondary, size: 24),
        const SizedBox(width: 14),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(title, style: TextStyle(fontSize: 17, color: color)),
              if (sub != null)
                Text(
                  sub,
                  style: const TextStyle(color: inkSecondary, fontSize: 14),
                ),
            ],
          ),
        ),
        ?trailing,
      ],
    ),
  );

  Widget get _divider => const Divider(height: 1, color: Color(0x14000000));

  @override
  Widget build(BuildContext context) {
    final nas = c.backend == Backend.nas;
    final email = c.googleEmail;
    final loggedIn = email != null && !c.authExpired;
    final counts = _counts;
    final written = c.macWrittenAt;
    final macText = written == null
        ? 'まだありません'
        : '${c.macWriter ?? 'Mac'} · ${clock(written)} に書き出し';
    return Container(
      color: groupedBackground,
      child: SafeArea(
        bottom: false,
        child: ListView(
          // The whole page is short enough to keep built.
          scrollCacheExtent: const ScrollCacheExtent.pixels(4000),
          padding: const EdgeInsets.only(bottom: 120),
          children: [
            const Padding(
              padding: EdgeInsets.fromLTRB(20, 24, 20, 0),
              child: Text(
                '設定',
                style: TextStyle(fontSize: 28, fontWeight: FontWeight.w800),
              ),
            ),
            _header('保存先'),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Segmented<Backend>(
                options: const [
                  (Backend.gdrive, 'Google ドライブ'),
                  (Backend.nas, 'NAS (WebDAV)'),
                ],
                value: c.backend,
                onChanged: c.setBackend,
              ),
            ),
            if (c.actionError != null && !nas)
              Padding(
                padding: const EdgeInsets.fromLTRB(20, 12, 20, 0),
                child: ErrorLine(c.actionError),
              ),
            if (!nas) ...[
              _header('GOOGLE アカウント'),
              _card([
                _row(
                  Icons.person_outline,
                  loggedIn ? email : 'ログインしていません',
                  sub: 'Google ドライブのアプリ専用の領域を使います',
                  trailing: loggedIn
                      ? TextButton(
                          onPressed: c.signOutGoogle,
                          child: const Text('ログアウト'),
                        )
                      : FilledButton(
                          onPressed: c.signInGoogle,
                          style: FilledButton.styleFrom(
                            backgroundColor: accent,
                          ),
                          child: const Text('Google でログイン'),
                        ),
                ),
              ]),
            ] else ...[
              _header('TAILSCALE'),
              _card([
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 12),
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: TailnetSection(controller: c),
                  ),
                ),
              ]),
              _header('NAS'),
              _card([
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 12),
                  child: NasForm(controller: c),
                ),
                _divider,
                TextButton(
                  onPressed: () => Navigator.of(context).push(
                    MaterialPageRoute<void>(
                      builder: (_) => Scaffold(
                        appBar: AppBar(),
                        body: SingleChildScrollView(
                          padding: const EdgeInsets.all(16),
                          child: NasGuide(controller: c),
                        ),
                      ),
                    ),
                  ),
                  child: const Text('NAS の準備を見る (初めての方へ)'),
                ),
              ]),
            ],
            _header('同期'),
            _card([
              _row(
                Icons.sync,
                '最終同期',
                trailing: Text(
                  c.syncing
                      ? '同期中…'
                      : c.lastSyncAt == null
                      ? '—'
                      : clock(c.lastSyncAt!),
                  style: const TextStyle(color: inkSecondary, fontSize: 17),
                ),
              ),
              _divider,
              _row(Icons.laptop_mac, 'Mac からの更新', sub: macText),
              _divider,
              _row(
                Icons.cloud_download_outlined,
                '今すぐ同期',
                trailing: FilledButton(
                  key: const Key('sync-now'),
                  onPressed: c.syncing ? null : c.sync,
                  style: FilledButton.styleFrom(backgroundColor: accent),
                  child: const Text('同期'),
                ),
              ),
            ]),
            if (c.syncError != null)
              Padding(
                padding: const EdgeInsets.fromLTRB(20, 10, 20, 0),
                child: ErrorLine(c.syncError),
              ),
            _footer('起動したとき、アプリに戻ったとき、下に引っ張ったときにも同期します。既読とスターは Mac にも反映されます。'),
            _header('端末内の控え'),
            _card([
              _row(
                Icons.storage_outlined,
                counts == null
                    ? '読み込み中…'
                    : '記事 ${_num(counts.items)}件 · ダイジェスト ${_num(counts.digests)}件',
              ),
              _divider,
              if (_confirmDelete)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 12),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      const Text('端末内の控えを消します。消しますか?'),
                      const SizedBox(height: 8),
                      Wrap(
                        spacing: 8,
                        children: [
                          FilledButton(
                            onPressed: () async {
                              await c.clearLocalCopy();
                              if (mounted) {
                                setState(() => _confirmDelete = false);
                              }
                            },
                            style: FilledButton.styleFrom(
                              backgroundColor: danger,
                            ),
                            child: const Text('消す'),
                          ),
                          OutlinedButton(
                            onPressed: () =>
                                setState(() => _confirmDelete = false),
                            child: const Text('やめる'),
                          ),
                        ],
                      ),
                    ],
                  ),
                )
              else
                InkWell(
                  onTap: () => setState(() => _confirmDelete = true),
                  child: _row(Icons.delete_outline, '控えを削除', color: danger),
                ),
            ]),
            _footer('削除しても保存先のデータは消えません。次の同期で取り直します。まだ送っていない既読とスターは失われます。'),
            _header('情報'),
            _card([
              _row(
                Icons.info_outline,
                'バージョン',
                trailing: Text(
                  c.version,
                  style: const TextStyle(color: inkSecondary, fontSize: 17),
                ),
              ),
            ]),
          ],
        ),
      ),
    );
  }

  static String _num(int n) => n.toString().replaceAllMapped(
    RegExp(r'\B(?=(\d{3})+(?!\d))'),
    (_) => ',',
  );
}

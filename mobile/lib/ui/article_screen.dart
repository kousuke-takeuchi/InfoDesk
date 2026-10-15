import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app/controller.dart';
import '../app/dates.dart';
import '../app/theme.dart';
import '../data/local_store.dart';
import '../sync/format.dart';
import 'widgets.dart';

/// M-3. Opening the screen marks the item read; so does 記事を開く.
///
/// Share: no share package is a dependency, so 共有 copies the URL.
class ArticleScreen extends StatefulWidget {
  const ArticleScreen({
    super.key,
    required this.controller,
    required this.itemId,
  });
  final AppController controller;
  final int itemId;

  @override
  State<ArticleScreen> createState() => _ArticleScreenState();
}

class _ArticleScreenState extends State<ArticleScreen> {
  StoredItem? _item;
  String _categoryLabel = '';
  String? _error;

  AppController get c => widget.controller;

  @override
  void initState() {
    super.initState();
    _init();
  }

  Future<void> _init() async {
    await _fetch();
    final i = _item;
    if (i != null && !i.isRead) {
      await c.setRead(i.record.id, true);
      await _fetch();
    }
  }

  Future<void> _fetch() async {
    final list = await c.store.items(filter: ItemFilter(ids: [widget.itemId]));
    final meta = await c.store.meta();
    if (!mounted) return;
    setState(() {
      _item = list.isEmpty ? null : list.first;
      final cat = _item?.record.category ?? '';
      _categoryLabel = cat;
      for (final m in meta?.categories ?? const <CategoryMeta>[]) {
        if (m.id == cat) _categoryLabel = m.label;
      }
    });
  }

  Future<void> _toggleStar() async {
    final i = _item!;
    await c.setStarred(i.record.id, !i.isStarred);
    await _fetch();
  }

  Future<void> _toggleRead() async {
    final i = _item!;
    await c.setRead(i.record.id, !i.isRead);
    await _fetch();
  }

  Future<void> _share() async {
    await Clipboard.setData(ClipboardData(text: _item!.record.url));
    if (!mounted) return;
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(const SnackBar(content: Text('リンクをコピーしました')));
  }

  Future<void> _openArticle() async {
    try {
      await c.openItem(_item!);
      await _fetch();
    } on Object {
      if (mounted) setState(() => _error = '記事を開けませんでした');
    }
  }

  @override
  Widget build(BuildContext context) {
    final i = _item;
    return Scaffold(
      appBar: AppBar(
        leadingWidth: 110,
        leading: TextButton.icon(
          onPressed: () => Navigator.of(context).maybePop(),
          icon: const Icon(Icons.chevron_left, size: 28),
          label: const Text('戻る', style: TextStyle(fontSize: 17)),
          style: TextButton.styleFrom(foregroundColor: accent),
        ),
        actions: [
          if (i != null) ...[
            IconButton(
              key: const Key('star-button'),
              tooltip: i.isStarred ? 'スターを外す' : 'スターを付ける',
              onPressed: _toggleStar,
              icon: Icon(
                i.isStarred ? Icons.star : Icons.star_border,
                color: starColor,
                size: 28,
              ),
            ),
            IconButton(
              key: const Key('read-button'),
              tooltip: i.isRead ? '未読にする' : '既読にする',
              onPressed: _toggleRead,
              icon: Icon(
                i.isRead
                    ? Icons.mark_email_unread_outlined
                    : Icons.drafts_outlined,
                color: inkSecondary,
                size: 26,
              ),
            ),
            IconButton(
              key: const Key('share-button'),
              tooltip: '共有',
              onPressed: _share,
              icon: const Icon(Icons.ios_share, color: inkSecondary, size: 26),
            ),
          ],
        ],
      ),
      body: i == null
          ? const SizedBox.shrink()
          : ListView(
              padding: const EdgeInsets.fromLTRB(20, 4, 20, 40),
              children: [
                if (RemoteImage.usable(i.record.imageUrl))
                  Container(
                    height: 210,
                    margin: const EdgeInsets.only(bottom: 16),
                    clipBehavior: Clip.antiAlias,
                    decoration: BoxDecoration(
                      color: fieldFill,
                      borderRadius: BorderRadius.circular(18),
                    ),
                    child: RemoteImage(i.record.imageUrl!),
                  ),
                Row(
                  children: [
                    Container(
                      padding: const EdgeInsets.symmetric(
                        horizontal: 10,
                        vertical: 5,
                      ),
                      decoration: BoxDecoration(
                        color: accentSoft,
                        borderRadius: BorderRadius.circular(14),
                      ),
                      child: Text(
                        _categoryLabel,
                        style: const TextStyle(
                          color: accent,
                          fontWeight: FontWeight.w700,
                          fontSize: 13,
                        ),
                      ),
                    ),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Text(
                        _meta(i),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(color: inkSecondary),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 14),
                Text(
                  i.record.title,
                  style: const TextStyle(
                    fontSize: 24,
                    fontWeight: FontWeight.w800,
                    height: 1.35,
                  ),
                ),
                const SizedBox(height: 18),
                if ((i.record.aiSummary ?? '').isNotEmpty)
                  Container(
                    padding: const EdgeInsets.all(16),
                    margin: const EdgeInsets.only(bottom: 16),
                    decoration: BoxDecoration(
                      color: groupedBackground,
                      borderRadius: BorderRadius.circular(16),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        const Row(
                          children: [
                            Icon(Icons.auto_awesome, size: 16, color: accent),
                            SizedBox(width: 6),
                            Text(
                              '要約',
                              style: TextStyle(
                                color: inkSecondary,
                                fontWeight: FontWeight.w700,
                              ),
                            ),
                          ],
                        ),
                        const SizedBox(height: 8),
                        Text(
                          i.record.aiSummary!,
                          style: const TextStyle(fontSize: 17, height: 1.55),
                        ),
                      ],
                    ),
                  ),
                if (i.record.excerpt.isNotEmpty)
                  Text(
                    i.record.excerpt,
                    style: const TextStyle(
                      fontSize: 16,
                      height: 1.6,
                      color: inkSecondary,
                    ),
                  ),
                const SizedBox(height: 20),
                ErrorLine(_error),
                FilledButton.icon(
                  onPressed: _openArticle,
                  icon: const Icon(Icons.public),
                  label: const Text(
                    '記事を開く',
                    style: TextStyle(fontSize: 18, fontWeight: FontWeight.w700),
                  ),
                  style: FilledButton.styleFrom(
                    backgroundColor: accent,
                    minimumSize: const Size.fromHeight(54),
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(16),
                    ),
                  ),
                ),
                const SizedBox(height: 12),
                const Text(
                  '開くとアプリ内のブラウザで表示し、既読にします。',
                  textAlign: TextAlign.center,
                  style: TextStyle(color: inkSecondary, fontSize: 13),
                ),
              ],
            ),
    );
  }

  String _meta(StoredItem i) {
    final r = i.record;
    final t = DateTime.tryParse(r.publishedAt ?? r.fetchedAt)?.toLocal();
    return t == null
        ? r.sourceName
        : '${r.sourceName} · ${relativeTime(t, c.now())}';
  }
}

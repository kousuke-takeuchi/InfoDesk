import 'package:flutter/material.dart';

import '../app/controller.dart';
import '../app/dates.dart';
import '../app/theme.dart';
import '../data/local_store.dart';
import '../sync/format.dart';
import 'article_screen.dart';
import 'widgets.dart';

enum _Range {
  today('今日', 0),
  threeDays('3日', 2),
  week('1週間', 6),
  month('1ヶ月', 29),
  all('すべて', null);

  const _Range(this.label, this.days);
  final String label;
  final int? days;
}

const _pageSize = 300;

/// M-2. With [starredOnly] it is the スター tab: the same list, starred
/// items only.
class FeedScreen extends StatefulWidget {
  const FeedScreen({
    super.key,
    required this.controller,
    this.starredOnly = false,
  });
  final AppController controller;
  final bool starredOnly;

  @override
  State<FeedScreen> createState() => _FeedScreenState();
}

class _FeedScreenState extends State<FeedScreen> {
  var _range = _Range.week;
  var _unreadOnly = false;
  var _starred = false;
  String? _category;
  int? _topic;
  var _query = '';
  List<StoredItem> _items = const [];
  var _unread = 0;
  MetaFile? _meta;
  var _loaded = false;
  int _seen = -1;
  int _load = 0;

  AppController get c => widget.controller;

  @override
  void initState() {
    super.initState();
    _starred = widget.starredOnly;
    c.addListener(_onController);
    _reload();
  }

  @override
  void dispose() {
    c.removeListener(_onController);
    super.dispose();
  }

  void _onController() {
    if (c.revision != _seen) _reload();
  }

  ItemFilter _filter({bool unread = false}) {
    final days = _range.days;
    return ItemFilter(
      periodStart: days == null
          ? null
          : dateString(dayOnly(c.now()).subtract(Duration(days: days))),
      category: _category,
      topicId: _topic,
      unreadOnly: unread || _unreadOnly,
      starredOnly: _starred,
      search: _query,
    );
  }

  Future<void> _reload() async {
    _seen = c.revision;
    final mine = ++_load;
    final items = await c.store.items(filter: _filter(), limit: _pageSize);
    final unread = await c.store.countItems(_filter(unread: true));
    final meta = await c.store.meta();
    if (!mounted || mine != _load) return;
    setState(() {
      _items = items;
      _unread = unread;
      _meta = meta;
      _loaded = true;
    });
  }

  String get _scopeLabel {
    if (_category != null) {
      for (final m in _meta?.categories ?? const <CategoryMeta>[]) {
        if (m.id == _category) return m.label;
      }
      return _category!;
    }
    if (_topic != null) {
      for (final t in _meta?.topics ?? const <TopicMeta>[]) {
        if (t.id == _topic) return '#${t.name}';
      }
      return 'トピック';
    }
    return 'すべて';
  }

  void _open(StoredItem i) => Navigator.of(context).push(
    MaterialPageRoute<void>(
      builder: (_) => ArticleScreen(controller: c, itemId: i.record.id),
    ),
  );

  void _cycleRange() {
    final all = _Range.values;
    setState(() => _range = all[(_range.index + 1) % all.length]);
    _reload();
  }

  Future<void> _pickScope() async {
    final meta = _meta;
    final picked = await showModalBottomSheet<(String?, int?)>(
      context: context,
      builder: (ctx) => SafeArea(
        child: ListView(
          shrinkWrap: true,
          children: [
            ListTile(
              title: const Text('すべて'),
              onTap: () => Navigator.pop(ctx, (null, null)),
            ),
            for (final m in meta?.categories ?? const <CategoryMeta>[])
              ListTile(
                title: Text(m.label),
                onTap: () => Navigator.pop(ctx, (m.id, null)),
              ),
            for (final t in meta?.topics ?? const <TopicMeta>[])
              ListTile(
                title: Text('#${t.name}'),
                onTap: () => Navigator.pop(ctx, (null, t.id)),
              ),
          ],
        ),
      ),
    );
    if (picked == null) return;
    setState(() {
      _category = picked.$1;
      _topic = picked.$2;
    });
    _reload();
  }

  @override
  Widget build(BuildContext context) {
    final title = widget.starredOnly ? 'スター' : 'フィード';
    final now = c.now();
    return SafeArea(
      bottom: false,
      child: RefreshIndicator(
        onRefresh: c.sync,
        child: ListView(
          physics: const AlwaysScrollableScrollPhysics(),
          padding: const EdgeInsets.fromLTRB(20, 16, 20, 110),
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.end,
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        _loaded ? '$_scopeLabel · 未読 $_unread件' : _scopeLabel,
                        style: const TextStyle(
                          color: inkSecondary,
                          fontSize: 15,
                        ),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        title,
                        style: const TextStyle(
                          fontSize: 28,
                          fontWeight: FontWeight.w800,
                        ),
                      ),
                    ],
                  ),
                ),
                if (_unread > 0)
                  IconButton(
                    key: const Key('mark-all-read'),
                    tooltip: 'すべて既読にする',
                    onPressed: () async {
                      await c.markAllRead(_filter());
                    },
                    icon: const Icon(Icons.done_all, color: accent),
                  ),
              ],
            ),
            const SizedBox(height: 12),
            TextField(
              onChanged: (v) {
                _query = v;
                _reload();
              },
              textInputAction: TextInputAction.search,
              decoration: InputDecoration(
                hintText: '記事を検索',
                prefixIcon: const Icon(Icons.search, color: inkSecondary),
                filled: true,
                fillColor: fieldFill,
                contentPadding: const EdgeInsets.symmetric(vertical: 12),
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(14),
                  borderSide: BorderSide.none,
                ),
              ),
            ),
            const SizedBox(height: 12),
            SizedBox(
              height: 40,
              child: ListView(
                scrollDirection: Axis.horizontal,
                children: [
                  FilterPill(
                    label: _range.label,
                    icon: Icons.calendar_today_outlined,
                    selected: true,
                    onTap: _cycleRange,
                  ),
                  const SizedBox(width: 8),
                  FilterPill(
                    label: '未読のみ',
                    icon: Icons.radio_button_checked,
                    selected: _unreadOnly,
                    onTap: () {
                      setState(() => _unreadOnly = !_unreadOnly);
                      _reload();
                    },
                  ),
                  if (!widget.starredOnly) ...[
                    const SizedBox(width: 8),
                    FilterPill(
                      label: 'スター',
                      icon: Icons.star_border,
                      selected: _starred,
                      onTap: () {
                        setState(() => _starred = !_starred);
                        _reload();
                      },
                    ),
                  ],
                  const SizedBox(width: 8),
                  FilterPill(
                    label: _category == null && _topic == null
                        ? 'カテゴリー'
                        : _scopeLabel,
                    icon: Icons.sell_outlined,
                    selected: _category != null || _topic != null,
                    onTap: _pickScope,
                  ),
                ],
              ),
            ),
            const SizedBox(height: 8),
            SyncNotice(c),
            if (_loaded && _items.isEmpty)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 40),
                child: Center(
                  child: Text(
                    widget.starredOnly ? 'スターを付けた記事はまだありません' : '記事がありません',
                    style: const TextStyle(color: inkSecondary),
                  ),
                ),
              ),
            for (final i in _items)
              _Row(item: i, now: now, onTap: () => _open(i)),
          ],
        ),
      ),
    );
  }
}

class _Row extends StatelessWidget {
  const _Row({required this.item, required this.now, required this.onTap});
  final StoredItem item;
  final DateTime now;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final r = item.record;
    final t = DateTime.tryParse(r.publishedAt ?? r.fetchedAt)?.toLocal();
    final unread = !item.isRead;
    final textColor = unread ? ink : inkSecondary;
    return InkWell(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(vertical: 16),
        decoration: const BoxDecoration(
          border: Border(bottom: BorderSide(color: Color(0x14000000))),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              width: 18,
              child: unread
                  ? Container(
                      width: 9,
                      height: 9,
                      margin: const EdgeInsets.only(top: 8),
                      decoration: const BoxDecoration(
                        color: accent,
                        shape: BoxShape.circle,
                      ),
                    )
                  : null,
            ),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    r.title,
                    maxLines: 3,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 17,
                      height: 1.3,
                      fontWeight: unread ? FontWeight.w700 : FontWeight.w600,
                      color: textColor,
                    ),
                  ),
                  const SizedBox(height: 4),
                  Row(
                    children: [
                      Flexible(
                        child: Text(
                          t == null
                              ? r.sourceName
                              : '${r.sourceName} · ${relativeTime(t, now)}',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(
                            color: inkSecondary,
                            fontSize: 13,
                          ),
                        ),
                      ),
                      if (item.isStarred) ...[
                        const SizedBox(width: 6),
                        const Icon(Icons.star, size: 14, color: starColor),
                      ],
                    ],
                  ),
                  if (r.excerpt.isNotEmpty) ...[
                    const SizedBox(height: 4),
                    Text(
                      r.excerpt,
                      maxLines: 3,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(
                        color: inkSecondary,
                        fontSize: 14,
                        height: 1.4,
                      ),
                    ),
                  ],
                ],
              ),
            ),
            if (RemoteImage.usable(r.imageUrl))
              Container(
                width: 72,
                height: 72,
                margin: const EdgeInsets.only(left: 12),
                clipBehavior: Clip.antiAlias,
                decoration: BoxDecoration(
                  color: fieldFill,
                  borderRadius: BorderRadius.circular(10),
                ),
                child: RemoteImage(r.imageUrl!),
              ),
          ],
        ),
      ),
    );
  }
}

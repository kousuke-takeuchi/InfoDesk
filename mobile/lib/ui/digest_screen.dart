import 'package:flutter/material.dart';

import '../app/controller.dart';
import '../app/dates.dart';
import '../app/digest_text.dart';
import '../app/theme.dart';
import '../data/local_store.dart';
import '../sync/format.dart';
import 'article_screen.dart';
import 'widgets.dart';

const _categoryLabels = {
  'github': 'GitHub',
  'arxiv': 'arXiv',
  'huggingface': 'Hugging Face',
  'hackernews': 'Hacker News',
  'tech_jp': '国内tech',
  'tech_intl': '海外tech',
  'medcare': '医療介護',
};

class _CategoryStat {
  _CategoryStat(this.category, this.count);
  final String category;
  final int count;
}

class _TopicStat {
  _TopicStat(this.name, this.count, this.prev);
  final String name;
  final int count;
  final int prev;
}

class _Stats {
  _Stats(this.total, this.categories, this.topics);
  final int total;
  final List<_CategoryStat> categories;
  final List<_TopicStat> topics;

  /// The Mac writes this JSON into the digest file. Anything unexpected
  /// gives no overview rather than an error.
  static _Stats? parse(Object? raw) {
    if (raw is! Map) return null;
    try {
      return _Stats(
        raw['total'] as int,
        [
          for (final c in (raw['categories'] as List? ?? const []))
            _CategoryStat((c as Map)['category'] as String, c['count'] as int),
        ],
        [
          for (final t in (raw['topics'] as List? ?? const []))
            _TopicStat(
              (t as Map)['name'] as String,
              t['count'] as int,
              t['prev_count'] as int,
            ),
        ],
      );
    } on Object {
      return null;
    }
  }
}

/// A digest scope: `{category: id}` or `{topic: id}`.
String _scopeKey(Object? scope) {
  if (scope is Map) {
    if (scope['category'] != null) return 'category-${scope['category']}';
    if (scope['topic'] != null) return 'topic-${scope['topic']}';
  }
  return 'all';
}

class DigestScreen extends StatefulWidget {
  const DigestScreen({super.key, required this.controller});
  final AppController controller;

  @override
  State<DigestScreen> createState() => _DigestScreenState();
}

class _DigestScreenState extends State<DigestScreen> {
  var _period = DigestPeriod.day;
  String _scope = 'all';
  DigestFile? _digest;
  MetaFile? _meta;
  Map<int, StoredItem> _refs = {};
  bool _loaded = false;
  int _seen = -1;
  int _load = 0;

  /// Images that failed to load; those items go back to the text links.
  final _brokenImages = <int>{};

  AppController get c => widget.controller;

  @override
  void initState() {
    super.initState();
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

  Future<void> _reload() async {
    _seen = c.revision;
    final mine = ++_load;
    final start = dateString(periodStart(_period, c.now()));
    final digest = await c.store.digests(_period.id, start);
    final meta = await c.store.meta();
    final ids = <int>{
      for (final e in digest?.entries ?? const <DigestRecord>[])
        ...e.itemIds.take(12),
    }.toList();
    final items = ids.isEmpty
        ? <StoredItem>[]
        : await c.store.items(filter: ItemFilter(ids: ids));
    if (!mounted || mine != _load) return;
    setState(() {
      _digest = digest;
      _meta = meta;
      _refs = {for (final i in items) i.record.id: i};
      _loaded = true;
    });
  }

  String _label(Object? scope) {
    if (scope is Map) {
      final cat = scope['category'];
      if (cat is String) {
        for (final m in _meta?.categories ?? const <CategoryMeta>[]) {
          if (m.id == cat) return m.label;
        }
        return _categoryLabels[cat] ?? cat;
      }
      final topic = scope['topic'];
      if (topic is int) {
        for (final t in _meta?.topics ?? const <TopicMeta>[]) {
          if (t.id == topic) return '#${t.name}';
        }
        return '#$topic';
      }
    }
    return 'すべて';
  }

  String _categoryName(String id) {
    for (final m in _meta?.categories ?? const <CategoryMeta>[]) {
      if (m.id == id) return m.label;
    }
    return _categoryLabels[id] ?? id;
  }

  void _openItem(StoredItem i) => Navigator.of(context).push(
    MaterialPageRoute<void>(
      builder: (_) => ArticleScreen(controller: c, itemId: i.record.id),
    ),
  );

  @override
  Widget build(BuildContext context) {
    final heading = periodHeading(_period, c.now());
    final entries = _digest?.entries ?? const <DigestRecord>[];
    final stats = _Stats.parse(_digest?.stats);
    final shown = [
      for (final e in entries)
        if (_scope == 'all' || _scopeKey(e.scope) == _scope) e,
    ];
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
                        heading.range,
                        style: const TextStyle(
                          color: inkSecondary,
                          fontSize: 15,
                        ),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        heading.title,
                        style: const TextStyle(
                          fontSize: 28,
                          fontWeight: FontWeight.w800,
                        ),
                      ),
                    ],
                  ),
                ),
                IconButton(
                  key: const Key('digest-sync'),
                  tooltip: '同期',
                  onPressed: c.syncing ? null : c.sync,
                  icon: c.syncing
                      ? const SizedBox(
                          width: 20,
                          height: 20,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.sync, color: inkSecondary),
                ),
              ],
            ),
            const SizedBox(height: 14),
            Segmented<DigestPeriod>(
              options: [for (final p in DigestPeriod.values) (p, p.label)],
              value: _period,
              onChanged: (p) {
                setState(() {
                  _period = p;
                  _scope = 'all';
                });
                _reload();
              },
            ),
            const SizedBox(height: 14),
            SyncNotice(c),
            if (entries.length > 1)
              SizedBox(
                height: 40,
                child: ListView(
                  scrollDirection: Axis.horizontal,
                  children: [
                    FilterPill(
                      label: 'すべて',
                      dark: true,
                      selected: _scope == 'all',
                      onTap: () => setState(() => _scope = 'all'),
                    ),
                    for (final e in entries) ...[
                      const SizedBox(width: 8),
                      FilterPill(
                        label: _label(e.scope),
                        dark: true,
                        selected: _scope == _scopeKey(e.scope),
                        onTap: () =>
                            setState(() => _scope = _scopeKey(e.scope)),
                      ),
                    ],
                  ],
                ),
              ),
            const SizedBox(height: 16),
            if (stats != null) _overview(stats),
            if (stats != null && stats.topics.isNotEmpty) _topics(stats),
            if (_loaded && shown.isEmpty)
              const Padding(
                padding: EdgeInsets.symmetric(vertical: 32),
                child: Center(
                  child: Text(
                    'この期間のダイジェストはまだありません',
                    style: TextStyle(color: inkSecondary),
                  ),
                ),
              ),
            for (final e in shown) _section(e),
          ],
        ),
      ),
    );
  }

  Widget _overview(_Stats s) {
    if (s.total == 0) {
      return const Padding(
        padding: EdgeInsets.only(bottom: 20),
        child: Text('この期間に記事がありません', style: TextStyle(color: inkSecondary)),
      );
    }
    final max = s.categories.fold<int>(1, (m, e) => e.count > m ? e.count : m);
    return Container(
      margin: const EdgeInsets.only(bottom: 20),
      padding: const EdgeInsets.all(18),
      decoration: BoxDecoration(
        color: groupedBackground,
        borderRadius: BorderRadius.circular(20),
      ),
      child: Column(
        children: [
          Row(
            children: [
              const Expanded(
                child: Text(
                  '期間の概観',
                  style: TextStyle(
                    color: inkSecondary,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              Text('${s.total}件', style: const TextStyle(color: inkSecondary)),
            ],
          ),
          const SizedBox(height: 10),
          for (final cat in s.categories)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 5),
              child: Row(
                children: [
                  SizedBox(
                    width: 112,
                    child: Text(
                      _categoryName(cat.category),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  Expanded(
                    child: ClipRRect(
                      borderRadius: BorderRadius.circular(4),
                      child: LinearProgressIndicator(
                        value: cat.count / max,
                        minHeight: 8,
                        color: accent,
                        backgroundColor: barTrack,
                      ),
                    ),
                  ),
                  SizedBox(
                    width: 40,
                    child: Text(
                      '${cat.count}',
                      textAlign: TextAlign.right,
                      style: const TextStyle(color: inkSecondary),
                    ),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }

  Widget _topics(_Stats s) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Padding(
            padding: EdgeInsets.only(bottom: 6),
            child: Text(
              '話題の強さ',
              style: TextStyle(
                color: inkSecondary,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
          for (final t in s.topics)
            Container(
              padding: const EdgeInsets.symmetric(vertical: 12),
              decoration: const BoxDecoration(
                border: Border(bottom: BorderSide(color: Color(0x14000000))),
              ),
              child: Row(
                children: [
                  Expanded(
                    child: Text(t.name, style: const TextStyle(fontSize: 17)),
                  ),
                  Text(
                    '${t.count}件',
                    style: const TextStyle(color: inkSecondary),
                  ),
                  const SizedBox(width: 10),
                  _delta(t.count, t.prev),
                ],
              ),
            ),
        ],
      ),
    );
  }

  Widget _delta(int count, int prev) {
    final d = count - prev;
    final (icon, color, text) = d > 0
        ? (Icons.trending_up, success, '+$d')
        : d < 0
        ? (Icons.trending_down, danger, '-${-d}')
        : (Icons.remove, inkSecondary, '±0');
    return SizedBox(
      width: 64,
      child: Row(
        mainAxisAlignment: MainAxisAlignment.end,
        children: [
          Icon(icon, size: 16, color: color),
          const SizedBox(width: 4),
          Text(
            text,
            style: TextStyle(color: color, fontWeight: FontWeight.w600),
          ),
        ],
      ),
    );
  }

  Widget _section(DigestRecord e) {
    final parsed = parseDigestBody(e.body);
    // Items in the digest's own order.
    final items = [
      for (final id in e.itemIds.take(12))
        if (_refs[id] != null) _refs[id]!,
    ];
    final cards = [
      for (final i in items)
        if (RemoteImage.usable(i.record.imageUrl) &&
            !_brokenImages.contains(i.record.id))
          i,
    ].take(3).toList();
    final links = [
      for (final i in items)
        if (!cards.contains(i)) i,
    ];
    final more = e.itemIds.length - cards.length - links.take(5).length;
    return Padding(
      padding: const EdgeInsets.only(bottom: 28),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.baseline,
            textBaseline: TextBaseline.alphabetic,
            children: [
              Text(
                _label(e.scope),
                style: const TextStyle(
                  fontSize: 22,
                  fontWeight: FontWeight.w800,
                ),
              ),
              const SizedBox(width: 10),
              Flexible(
                child: Text(
                  '${e.provider} · 記事${e.itemIds.length}件',
                  style: const TextStyle(color: inkSecondary, fontSize: 13),
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          if (parsed.headline != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 12),
              child: Text(
                parsed.headline!,
                style: const TextStyle(
                  fontSize: 21,
                  fontWeight: FontWeight.w800,
                ),
              ),
            ),
          for (var i = 0; i < parsed.bullets.length; i++)
            Padding(
              padding: const EdgeInsets.only(bottom: 12),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Container(
                    width: 26,
                    height: 26,
                    margin: const EdgeInsets.only(right: 12, top: 1),
                    alignment: Alignment.center,
                    decoration: const BoxDecoration(
                      color: accentSoft,
                      shape: BoxShape.circle,
                    ),
                    child: Text(
                      '${i + 1}',
                      style: const TextStyle(
                        color: accent,
                        fontSize: 13,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ),
                  Expanded(
                    child: Text(
                      parsed.bullets[i],
                      style: const TextStyle(fontSize: 17, height: 1.5),
                    ),
                  ),
                ],
              ),
            ),
          for (final p in parsed.paragraphs)
            Padding(
              padding: const EdgeInsets.only(bottom: 10),
              child: Text(p, style: const TextStyle(fontSize: 16, height: 1.6)),
            ),
          if (cards.isNotEmpty)
            SizedBox(
              height: 230,
              child: ListView.separated(
                scrollDirection: Axis.horizontal,
                itemCount: cards.length,
                separatorBuilder: (_, _) => const SizedBox(width: 12),
                itemBuilder: (_, i) => _card(cards[i]),
              ),
            ),
          if (links.isNotEmpty || more > 0) ...[
            const SizedBox(height: 12),
            const Text(
              '根拠記事',
              style: TextStyle(color: inkSecondary, fontSize: 12),
            ),
            for (final i in links.take(5))
              GestureDetector(
                onTap: () => _openItem(i),
                child: Padding(
                  padding: const EdgeInsets.only(top: 6),
                  child: Text(
                    i.record.title,
                    style: const TextStyle(color: accent, fontSize: 15),
                  ),
                ),
              ),
            if (more > 0)
              Padding(
                padding: const EdgeInsets.only(top: 6),
                child: Text(
                  '他 $more件',
                  style: const TextStyle(color: inkSecondary),
                ),
              ),
          ],
        ],
      ),
    );
  }

  Widget _card(StoredItem i) {
    return GestureDetector(
      onTap: () => _openItem(i),
      child: Container(
        width: 250,
        clipBehavior: Clip.antiAlias,
        decoration: BoxDecoration(
          color: surface,
          borderRadius: BorderRadius.circular(18),
          border: Border.all(color: const Color(0x1F000000)),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              height: 130,
              width: double.infinity,
              child: ColoredBox(
                color: fieldFill,
                child: RemoteImage(
                  i.record.imageUrl!,
                  onError: () {
                    if (mounted) {
                      setState(() => _brokenImages.add(i.record.id));
                    }
                  },
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.all(12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    i.record.title,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontWeight: FontWeight.w700,
                      fontSize: 15,
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    i.record.sourceName,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(color: inkSecondary, fontSize: 13),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

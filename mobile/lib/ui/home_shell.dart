import 'package:flutter/material.dart';

import '../app/controller.dart';
import '../app/theme.dart';
import 'digest_screen.dart';
import 'feed_screen.dart';
import 'settings_screen.dart';

/// The four tabs under a floating, rounded bar.
class HomeShell extends StatefulWidget {
  const HomeShell({super.key, required this.controller});
  final AppController controller;

  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  var _index = 0;

  static const _tabs = [
    ('digest', 'ダイジェスト', Icons.newspaper_outlined, Icons.newspaper),
    ('feed', 'フィード', Icons.format_list_bulleted, Icons.format_list_bulleted),
    ('starred', 'スター', Icons.star_border, Icons.star),
    ('settings', '設定', Icons.settings_outlined, Icons.settings),
  ];

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    return Scaffold(
      backgroundColor: surface,
      body: IndexedStack(
        index: _index,
        children: [
          DigestScreen(controller: c),
          FeedScreen(controller: c),
          FeedScreen(controller: c, starredOnly: true),
          SettingsScreen(controller: c),
        ],
      ),
      bottomNavigationBar: SafeArea(
        child: Container(
          margin: const EdgeInsets.fromLTRB(16, 0, 16, 8),
          padding: const EdgeInsets.all(6),
          decoration: BoxDecoration(
            color: surface,
            borderRadius: BorderRadius.circular(32),
            border: Border.all(color: const Color(0x14000000)),
            boxShadow: const [
              BoxShadow(
                color: Color(0x1F000000),
                blurRadius: 18,
                offset: Offset(0, 4),
              ),
            ],
          ),
          child: Row(
            children: [
              for (var i = 0; i < _tabs.length; i++)
                Expanded(
                  child: Semantics(
                    button: true,
                    selected: i == _index,
                    label: _tabs[i].$2,
                    excludeSemantics: true,
                    onTap: () => setState(() => _index = i),
                    child: GestureDetector(
                      key: Key('tab-${_tabs[i].$1}'),
                      behavior: HitTestBehavior.opaque,
                      onTap: () => setState(() => _index = i),
                      child: AnimatedContainer(
                        duration: const Duration(milliseconds: 150),
                        padding: const EdgeInsets.symmetric(vertical: 8),
                        decoration: BoxDecoration(
                          color: i == _index ? accentSoft : Colors.transparent,
                          borderRadius: BorderRadius.circular(26),
                        ),
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Icon(
                              i == _index ? _tabs[i].$4 : _tabs[i].$3,
                              size: 24,
                              color: i == _index ? accent : inkSecondary,
                            ),
                            const SizedBox(height: 2),
                            Text(
                              _tabs[i].$2,
                              style: TextStyle(
                                fontSize: 11,
                                fontWeight: i == _index
                                    ? FontWeight.w600
                                    : FontWeight.w400,
                                color: i == _index ? accent : inkSecondary,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

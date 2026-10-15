import 'package:flutter/material.dart';

import '../app/controller.dart';
import '../app/theme.dart';
import '../settings/app_settings.dart';
import 'nas_form.dart';
import 'tailnet_section.dart';
import 'widgets.dart';

/// M-5: choose the storage and log in to it.
class FirstLaunchScreen extends StatelessWidget {
  const FirstLaunchScreen({super.key, required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final nas = c.backend == Backend.nas;
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Container(
                  width: 72,
                  height: 72,
                  decoration: BoxDecoration(
                    color: accentSoft,
                    borderRadius: BorderRadius.circular(22),
                  ),
                  child: const Icon(Icons.newspaper, color: accent, size: 36),
                ),
                const SizedBox(height: 20),
                const Text(
                  'InfoDesk',
                  style: TextStyle(fontSize: 30, fontWeight: FontWeight.w800),
                ),
                const SizedBox(height: 12),
                Text(
                  nas
                      ? 'Mac の InfoDesk が NAS に置いた記事とダイジェストを読めます。Tailscale にログインして、NAS の情報を入れてください。'
                      : 'Mac の InfoDesk が集めた記事とダイジェストを読めます。Mac と同じ Google アカウントでログインしてください。',
                  textAlign: TextAlign.center,
                  style: const TextStyle(
                    color: inkSecondary,
                    fontSize: 16,
                    height: 1.5,
                  ),
                ),
                const SizedBox(height: 20),
                Segmented<Backend>(
                  options: const [
                    (Backend.gdrive, 'Google ドライブ'),
                    (Backend.nas, 'NAS (WebDAV)'),
                  ],
                  value: c.backend,
                  onChanged: c.setBackend,
                ),
                const SizedBox(height: 20),
                if (!nas) ...[
                  ErrorLine(c.actionError),
                  SizedBox(
                    width: double.infinity,
                    child: FilledButton.icon(
                      onPressed: c.signInGoogle,
                      icon: const Icon(Icons.login),
                      label: const Text(
                        'Google でログイン',
                        style: TextStyle(
                          fontSize: 18,
                          fontWeight: FontWeight.w700,
                        ),
                      ),
                      style: FilledButton.styleFrom(
                        backgroundColor: accent,
                        minimumSize: const Size.fromHeight(54),
                        shape: RoundedRectangleBorder(
                          borderRadius: BorderRadius.circular(16),
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(height: 14),
                  const Text(
                    'Mac で同期をオンにしていないと、データはまだありません。',
                    textAlign: TextAlign.center,
                    style: TextStyle(color: inkSecondary, fontSize: 13),
                  ),
                ] else ...[
                  Align(
                    alignment: Alignment.centerLeft,
                    child: TailnetSection(controller: c),
                  ),
                  const SizedBox(height: 16),
                  ListenableBuilder(
                    listenable: c,
                    builder: (_, _) => NasForm(controller: c),
                  ),
                  const SizedBox(height: 16),
                  NasGuide(controller: c),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

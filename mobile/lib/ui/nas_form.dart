import 'package:flutter/material.dart';

import '../app/controller.dart';
import '../app/theme.dart';
import '../settings/nas_guide.dart';
import 'widgets.dart';

/// NAS URL, user, password and 確かめる. The fields are saved when they lose
/// focus and before a check.
class NasForm extends StatefulWidget {
  const NasForm({super.key, required this.controller});
  final AppController controller;

  @override
  State<NasForm> createState() => _NasFormState();
}

class _NasFormState extends State<NasForm> {
  late final _url = TextEditingController(text: c.nasConfig.url);
  late final _user = TextEditingController(text: c.nasConfig.user);
  final _password = TextEditingController();
  final _focus = [FocusNode(), FocusNode(), FocusNode()];
  var _confirmClear = false;
  var _skipBlur = false;

  AppController get c => widget.controller;

  @override
  void initState() {
    super.initState();
    for (final f in _focus) {
      f.addListener(() {
        if (f.hasFocus) return;
        if (_skipBlur) {
          _skipBlur = false;
          return;
        }
        _save();
      });
    }
  }

  @override
  void dispose() {
    for (final f in _focus) {
      f.dispose();
    }
    _url.dispose();
    _user.dispose();
    _password.dispose();
    super.dispose();
  }

  /// A blank password keeps the saved one.
  Future<void> _save() async {
    final pw = _password.text;
    await c.saveNas(
      url: _url.text,
      user: _user.text,
      password: pw.isEmpty ? null : pw,
    );
    if (!mounted) return;
    if (c.actionError == null) _password.clear();
  }

  Future<void> _check() async {
    // Dropping focus would save again; save once, here.
    final focused = _focus.where((f) => f.hasFocus).toList();
    if (focused.isNotEmpty) {
      _skipBlur = true;
      focused.first.unfocus();
    }
    await _save();
    if (!mounted) return;
    if (c.actionError == null) await c.checkNas();
  }

  Widget _field(
    String label,
    TextEditingController ctl,
    FocusNode node,
    Key key, {
    String? hint,
    bool secret = false,
  }) => Padding(
    padding: const EdgeInsets.only(bottom: 10),
    child: TextField(
      key: key,
      controller: ctl,
      focusNode: node,
      obscureText: secret,
      autocorrect: false,
      enableSuggestions: false,
      keyboardType: secret ? TextInputType.text : TextInputType.url,
      decoration: InputDecoration(
        labelText: label,
        hintText: hint,
        filled: true,
        fillColor: fieldFill,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(12),
          borderSide: BorderSide.none,
        ),
      ),
    ),
  );

  @override
  Widget build(BuildContext context) {
    final nasSaved =
        c.nasConfig.url.isNotEmpty ||
        c.nasConfig.user.isNotEmpty ||
        (c.nasConfig.password ?? '').isNotEmpty;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _field(
          'NAS の URL',
          _url,
          _focus[0],
          const Key('nas-url'),
          hint: 'http://takeuchi-nas:5005/infodesk/',
        ),
        _field('ユーザー名', _user, _focus[1], const Key('nas-user')),
        _field(
          'パスワード',
          _password,
          _focus[2],
          const Key('nas-password'),
          secret: true,
          hint: (c.nasConfig.password ?? '').isEmpty ? null : '保存済み',
        ),
        ErrorLine(c.actionError),
        Row(
          children: [
            FilledButton(
              onPressed: c.nasChecking ? null : _check,
              style: FilledButton.styleFrom(backgroundColor: accent),
              child: Text(c.nasChecking ? '確かめています…' : '確かめる'),
            ),
            const SizedBox(width: 12),
            if (c.nasCheckText != null)
              Expanded(
                child: Row(
                  children: [
                    Icon(
                      c.nasCheckPassed
                          ? Icons.check_circle
                          : Icons.error_outline,
                      size: 18,
                      color: c.nasCheckPassed ? success : danger,
                    ),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Text(
                        c.nasCheckText!,
                        style: TextStyle(
                          color: c.nasCheckPassed ? success : danger,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
          ],
        ),
        if (nasSaved) ...[
          const SizedBox(height: 8),
          if (_confirmClear) ...[
            const Text('NAS の URL、ユーザー名、パスワードを消します。消しますか?'),
            Wrap(
              spacing: 8,
              children: [
                FilledButton(
                  onPressed: () async {
                    await c.clearNas();
                    if (!mounted) return;
                    _url.clear();
                    _user.clear();
                    if (mounted) setState(() => _confirmClear = false);
                  },
                  style: FilledButton.styleFrom(backgroundColor: danger),
                  child: const Text('消す'),
                ),
                OutlinedButton(
                  onPressed: () => setState(() => _confirmClear = false),
                  child: const Text('やめる'),
                ),
              ],
            ),
          ] else
            TextButton(
              onPressed: () => setState(() => _confirmClear = true),
              child: const Text('NAS の設定を消す'),
            ),
        ],
      ],
    );
  }
}

/// A-15 as a phone page, built from the shared guide data.
class NasGuide extends StatelessWidget {
  const NasGuide({super.key, required this.controller});
  final AppController controller;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: groupedBackground,
        borderRadius: BorderRadius.circular(16),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text(
            nasGuideTitle,
            style: TextStyle(fontSize: 18, fontWeight: FontWeight.w800),
          ),
          const SizedBox(height: 6),
          const Text(nasGuideIntro, style: TextStyle(color: inkSecondary)),
          const SizedBox(height: 12),
          for (var i = 0; i < nasGuideSteps.length; i++)
            Padding(
              padding: const EdgeInsets.only(bottom: 12),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Container(
                    width: 24,
                    height: 24,
                    margin: const EdgeInsets.only(right: 10),
                    alignment: Alignment.center,
                    decoration: const BoxDecoration(
                      color: accentSoft,
                      shape: BoxShape.circle,
                    ),
                    child: Text(
                      '${i + 1}',
                      style: const TextStyle(
                        color: accent,
                        fontSize: 12,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          nasGuideSteps[i].title,
                          style: const TextStyle(fontWeight: FontWeight.w700),
                        ),
                        const SizedBox(height: 2),
                        Text(
                          nasGuideSteps[i].body,
                          style: const TextStyle(
                            color: inkSecondary,
                            fontSize: 14,
                            height: 1.45,
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          const Text(
            nasGuideTip,
            style: TextStyle(fontSize: 13, color: inkSecondary, height: 1.45),
          ),
          TextButton(
            onPressed: () => controller.openExternal(Uri.parse(nasGuideUrl)),
            child: const Text(nasGuideLinkLabel),
          ),
        ],
      ),
    );
  }
}

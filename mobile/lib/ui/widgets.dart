import 'package:flutter/material.dart';

import '../app/controller.dart';
import '../app/theme.dart';
import '../settings/app_settings.dart';

/// Pill-shaped segmented control (M-1 period, M-5 storage choice).
class Segmented<T> extends StatelessWidget {
  const Segmented({
    super.key,
    required this.options,
    required this.value,
    required this.onChanged,
  });

  final List<(T, String)> options;
  final T value;
  final ValueChanged<T> onChanged;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(3),
      decoration: BoxDecoration(
        color: fieldFill,
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        children: [
          for (final (v, label) in options)
            Expanded(
              child: Semantics(
                button: true,
                selected: v == value,
                child: GestureDetector(
                  behavior: HitTestBehavior.opaque,
                  onTap: () => onChanged(v),
                  child: AnimatedContainer(
                    duration: const Duration(milliseconds: 120),
                    padding: const EdgeInsets.symmetric(vertical: 9),
                    decoration: BoxDecoration(
                      color: v == value ? surface : Colors.transparent,
                      borderRadius: BorderRadius.circular(10),
                      boxShadow: v == value
                          ? const [
                              BoxShadow(
                                color: Color(0x1A000000),
                                blurRadius: 3,
                              ),
                            ]
                          : null,
                    ),
                    alignment: Alignment.center,
                    child: Text(
                      label,
                      style: TextStyle(
                        fontSize: 15,
                        fontWeight: v == value
                            ? FontWeight.w600
                            : FontWeight.w400,
                      ),
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// A filter chip. [selected] uses the accent tint (feed) or ink (digest).
class FilterPill extends StatelessWidget {
  const FilterPill({
    super.key,
    required this.label,
    required this.onTap,
    this.selected = false,
    this.icon,
    this.dark = false,
  });

  final String label;
  final VoidCallback onTap;
  final bool selected;
  final IconData? icon;
  final bool dark;

  @override
  Widget build(BuildContext context) {
    final bg = selected ? (dark ? ink : accentSoft) : fieldFill;
    final fg = selected ? (dark ? Colors.white : accent) : ink;
    return Semantics(
      button: true,
      selected: selected,
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
          decoration: BoxDecoration(
            color: bg,
            borderRadius: BorderRadius.circular(20),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (icon != null) ...[
                Icon(icon, size: 16, color: fg),
                const SizedBox(width: 6),
              ],
              Text(
                label,
                style: TextStyle(
                  color: fg,
                  fontSize: 14,
                  fontWeight: selected ? FontWeight.w600 : FontWeight.w400,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class ErrorLine extends StatelessWidget {
  const ErrorLine(this.message, {super.key, this.actionLabel, this.onAction});
  final String? message;

  /// An inline button after the message, e.g. 「Google でログイン」.
  final String? actionLabel;
  final VoidCallback? onAction;

  @override
  Widget build(BuildContext context) {
    final m = message;
    if (m == null) return const SizedBox.shrink();
    final line = Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Icon(Icons.error_outline, size: 18, color: danger),
        const SizedBox(width: 8),
        Expanded(
          child: Text(m, style: const TextStyle(color: danger, fontSize: 14)),
        ),
      ],
    );
    final label = actionLabel;
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: label == null || onAction == null
          ? line
          : Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                line,
                TextButton(onPressed: onAction, child: Text(label)),
              ],
            ),
    );
  }
}

/// The sync problem and warnings, shown above the lists.
class SyncNotice extends StatelessWidget {
  const SyncNotice(this.controller, {super.key});
  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    // Only a pass that found no usable login asks for one: a network error
    // with no email yet (an offline launch) is retried, not a lost login.
    final needsLogin = c.backend == Backend.gdrive && c.authExpired;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        ErrorLine(
          c.syncError,
          actionLabel: needsLogin ? 'Google でログイン' : null,
          onAction: needsLogin ? c.signInGoogle : null,
        ),
        for (final w in c.warnings)
          Padding(
            padding: const EdgeInsets.only(bottom: 6),
            child: Text(
              w,
              style: const TextStyle(color: inkSecondary, fontSize: 13),
            ),
          ),
      ],
    );
  }
}

class RemoteImage extends StatelessWidget {
  const RemoteImage(
    this.url, {
    super.key,
    this.fit = BoxFit.cover,
    this.onError,
  });
  final String url;
  final BoxFit fit;

  /// Called (after the frame) when the image cannot be loaded.
  final VoidCallback? onError;

  /// Only https images are fetched, as on the Mac.
  static bool usable(String? url) =>
      url != null && url.toLowerCase().startsWith('https://');

  @override
  Widget build(BuildContext context) => Image.network(
    url,
    fit: fit,
    errorBuilder: (_, _, _) {
      final cb = onError;
      if (cb != null) WidgetsBinding.instance.addPostFrameCallback((_) => cb());
      return const SizedBox.shrink();
    },
    loadingBuilder: (c, child, p) =>
        p == null ? child : const ColoredBox(color: fieldFill),
  );
}

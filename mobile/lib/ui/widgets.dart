import 'package:flutter/material.dart';

import '../app/controller.dart';
import '../app/theme.dart';

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
                            BoxShadow(color: Color(0x1A000000), blurRadius: 3),
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
    return GestureDetector(
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
    );
  }
}

class ErrorLine extends StatelessWidget {
  const ErrorLine(this.message, {super.key});
  final String? message;

  @override
  Widget build(BuildContext context) {
    final m = message;
    if (m == null) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Icon(Icons.error_outline, size: 18, color: danger),
          const SizedBox(width: 8),
          Expanded(
            child: Text(m, style: const TextStyle(color: danger, fontSize: 14)),
          ),
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
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        ErrorLine(controller.syncError),
        for (final w in controller.warnings)
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
  const RemoteImage(this.url, {super.key, this.fit = BoxFit.cover});
  final String url;
  final BoxFit fit;

  /// Only https images are fetched, as on the Mac.
  static bool usable(String? url) =>
      url != null && url.toLowerCase().startsWith('https://');

  @override
  Widget build(BuildContext context) => Image.network(
    url,
    fit: fit,
    errorBuilder: (_, _, _) => const SizedBox.shrink(),
    loadingBuilder: (c, child, p) =>
        p == null ? child : const ColoredBox(color: fieldFill),
  );
}

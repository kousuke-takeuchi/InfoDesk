import 'package:flutter/material.dart';

const accent = Color(0xFF0A6CFF);
const accentSoft = Color(0xFFE1EBFF);
const ink = Color(0xFF1C1C1E);
const inkSecondary = Color(0xFF8A8A8E);
const surface = Color(0xFFFFFFFF);
const groupedBackground = Color(0xFFF2F2F7);
const fieldFill = Color(0xFFF2F2F5);
const barTrack = Color(0xFFE3E3E6);
const danger = Color(0xFFE5321F);
const success = Color(0xFF1FA14A);
const starColor = Color(0xFFF5A623);

ThemeData buildTheme() {
  final scheme = ColorScheme.fromSeed(
    seedColor: accent,
    brightness: Brightness.light,
  ).copyWith(primary: accent, surface: surface, onSurface: ink);
  return ThemeData(
    useMaterial3: true,
    colorScheme: scheme,
    scaffoldBackgroundColor: surface,
    splashFactory: NoSplash.splashFactory,
    appBarTheme: const AppBarTheme(
      backgroundColor: surface,
      foregroundColor: ink,
      elevation: 0,
      scrolledUnderElevation: 0,
    ),
    textTheme: Typography.blackCupertino.apply(
      bodyColor: ink,
      displayColor: ink,
    ),
  );
}

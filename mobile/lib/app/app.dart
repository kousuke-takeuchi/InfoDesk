import 'package:flutter/material.dart';

import '../ui/first_launch.dart';
import '../ui/home_shell.dart';
import 'controller.dart';
import 'theme.dart';

class InfoDeskApp extends StatefulWidget {
  const InfoDeskApp({super.key, required this.controller});
  final AppController controller;

  @override
  State<InfoDeskApp> createState() => _InfoDeskAppState();
}

class _InfoDeskAppState extends State<InfoDeskApp> {
  late final AppLifecycleListener _lifecycle;

  @override
  void initState() {
    super.initState();
    // A pass on every return to the app.
    _lifecycle = AppLifecycleListener(
      onResume: () {
        final c = widget.controller;
        if (c.ready && c.onboarded) c.sync();
      },
    );
  }

  @override
  void dispose() {
    _lifecycle.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    return MaterialApp(
      title: 'InfoDesk',
      debugShowCheckedModeBanner: false,
      theme: buildTheme(),
      home: ListenableBuilder(
        listenable: c,
        builder: (context, _) {
          if (!c.ready) {
            return const Scaffold(body: SizedBox.expand());
          }
          if (!c.onboarded) return FirstLaunchScreen(controller: c);
          return HomeShell(controller: c);
        },
      ),
    );
  }
}

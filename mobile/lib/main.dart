import 'package:flutter/material.dart';

void main() {
  runApp(const InfoDeskApp());
}

class InfoDeskApp extends StatelessWidget {
  const InfoDeskApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'InfoDesk',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.indigo),
      ),
      home: const Scaffold(body: Center(child: Text('InfoDesk'))),
    );
  }
}

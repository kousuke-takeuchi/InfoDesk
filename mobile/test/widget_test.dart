import 'package:flutter_test/flutter_test.dart';
import 'package:infodesk_mobile/main.dart';
import 'package:infodesk_mobile/packages.dart';

void main() {
  testWidgets('shows the app name', (tester) async {
    await tester.pumpWidget(const InfoDeskApp());
    expect(find.text('InfoDesk'), findsOneWidget);
  });

  test('links every package from the tech stack', () {
    expect(linkedPackages(), hasLength(10));
  });
}

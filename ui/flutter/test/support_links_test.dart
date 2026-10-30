import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:peridot_essential_ui/main.dart';

void main() {
  testWidgets('about page carries the community build and the upstream credit', (
    WidgetTester tester,
  ) async {
    tester.view.physicalSize = const Size(430, 1400);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      const MaterialApp(home: Scaffold(body: SupportScreen())),
    );
    await tester.pump();

    // GitHub card badge plus the "Crafted for the community by" footer.
    expect(find.text('afrizal1221'), findsNWidgets(2));
    expect(find.text('@PocoF6Indo'), findsOneWidget);
    expect(
      find.text(
        'Built on Rodin Essential by NEESCHAL-3 · Peridot adaptation by davvv98',
      ),
      findsOneWidget,
    );

    // The old community build's chat and the bare author badge are gone.
    expect(find.text('@PocoX7ProNepalChat'), findsNothing);
    expect(find.text('NEESCHAL-3'), findsNothing);

    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}

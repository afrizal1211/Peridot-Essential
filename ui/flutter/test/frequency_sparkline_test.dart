import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:peridot_essential_ui/main.dart';

void main() {
  testWidgets(
    'PeridotFrequencySparkline draws once history exists and skips bad samples',
    (WidgetTester tester) async {
      final List<double> samples = <double>[300, 600, 900, 1150];
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: PeridotFrequencySparkline(
              samples: samples,
              minHz: 264,
              maxHz: 1150,
            ),
          ),
        ),
      );
      expect(find.byType(CustomPaint), findsOneWidget);

      // A fresh widget fed only invalid samples stays empty (no line drawn).
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: PeridotFrequencySparkline(
              samples: const <double>[0, -5],
              minHz: 264,
              maxHz: 1150,
            ),
          ),
        ),
      );
      expect(find.byType(CustomPaint), findsNothing);
    },
  );
}

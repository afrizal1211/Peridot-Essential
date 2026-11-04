import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:peridot_essential_ui/main.dart';

void main() {
  // Fresh state per pump: a same-type widget at the same position would reuse
  // the element (and its history), which would defeat the empty-input case.
  Future<void> pump(
    WidgetTester tester,
    Key key,
    List<double> samples,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: PeridotFrequencySparkline(
            key: key,
            samples: samples,
            minHz: 264,
            maxHz: 1150,
          ),
        ),
      ),
    );
  }

  testWidgets(
    'PeridotFrequencySparkline draws once history exists and skips bad samples',
    (WidgetTester tester) async {
      await pump(
        tester,
        const ValueKey<int>(1),
        <double>[300, 600, 900, 1150],
      );
      expect(find.byType(CustomPaint), findsOneWidget);

      await pump(tester, const ValueKey<int>(2), const <double>[0, -5]);
      expect(find.byType(CustomPaint), findsNothing);
    },
  );
}

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:peridot_essential_ui/main.dart';

// MaterialApp/Scaffold inject their own CustomPaints, which would pollute a
// byType lookup; match the sparkline's painter instead.
final Finder sparklinePaint = find.byWidgetPredicate(
  (Widget w) => w is CustomPaint && w.painter != null && w.size.height == 64,
);

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
            color: const Color(0xFF67C2FF),
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
      expect(sparklinePaint, findsOneWidget);

      await pump(tester, const ValueKey<int>(2), const <double>[0, -5]);
      expect(sparklinePaint, findsNothing);
    },
  );
}

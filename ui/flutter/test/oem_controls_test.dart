import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:peridot_essential_ui/main.dart';

void main() {
  testWidgets('hero keeps full descriptions at narrow width', (tester) async {
    const description =
        'Peridot Essential override released · ROM-managed timing · Goodix GT9916 · Vendor HAL ready';
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: SizedBox(
            width: 280,
            child: HeroCard(
              icon: Icons.auto_awesome_rounded,
              accent: Colors.blue,
              title: 'OEM Control',
              subtitle: description,
            ),
          ),
        ),
      ),
    );
    final paragraph = tester.renderObject<RenderParagraph>(
      find.text(description),
    );
    expect(paragraph.didExceedMaxLines, isFalse);
    expect(tester.takeException(), isNull);
  });

  testWidgets('touch offers OEM without removing fixed modes', (tester) async {
    tester.view.physicalSize = const Size(430, 1100);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(body: TouchBoostScreen(onBack: () {})),
      ),
    );
    await tester.pump();
    expect(find.text('OEM Control'), findsNWidgets(2));
    expect(find.text('Default · Game mode off'), findsOneWidget);
    expect(find.text('250 Hz'), findsOneWidget);
    expect(find.text('480 Hz'), findsOneWidget);
    // This test attaches no TouchFeature HAL, so the screen must say the
    // engine is unavailable instead of advertising a working rate.
    expect(find.text('Touch control unavailable'), findsOneWidget);
    expect(
      find.textContaining('TouchFeature AIDL service is not exposed'),
      findsOneWidget,
    );
    final scroll = tester.state<ScrollableState>(find.byType(Scrollable).first);
    scroll.position.jumpTo(scroll.position.maxScrollExtent);
    await tester.pumpAndSettle();
    expect(find.text('Double tap to wake'), findsNothing);
    expect(find.byIcon(Icons.wb_twilight_rounded), findsNothing);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}

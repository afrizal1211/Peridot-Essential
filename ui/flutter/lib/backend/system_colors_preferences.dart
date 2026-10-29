import 'dart:convert';
import 'dart:io';

import 'system_colors_state.dart';

/// A UI-only hint. Android remains the owner of the actual palette.
final class PeridotSystemColorsPreferences {
  static File get _file => File(
    '${Directory.systemTemp.parent.path}/files/peridot-system-colors.json',
  );

  static Future<PeridotSystemColorsSelection?> read() async {
    try {
      final File file = _file;
      if (!await file.exists() || await file.length() > 1024) return null;
      return PeridotSystemColorsSelection.fromJson(
        jsonDecode(await file.readAsString()),
      );
    } catch (_) {
      return null;
    }
  }

  static Future<void> write(PeridotSystemColorsSelection? selection) async {
    try {
      final File file = _file;
      await file.parent.create(recursive: true);
      final File pending = File('${file.path}.pending');
      await pending.writeAsString(
        jsonEncode(selection?.toJson()),
        flush: false,
      );
      await pending.rename(file.path);
    } catch (_) {
      // A missing UI hint must not affect Android's saved palette.
    }
  }
}

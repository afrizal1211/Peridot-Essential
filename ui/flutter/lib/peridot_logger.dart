// Peridot logger — in-memory ring buffer, accessible in-app.
//
// Skipped: disk persistence, log export (add when user requests).
// Dart runs single-isolate; no cross-thread locking needed for simple list ops.

import 'package:flutter/foundation.dart';

enum PeridotLogLevel {
  ok('✓', 0xFF4CAF50),
  info('i', 0xFF64B5F6),
  warn('!', 0xFFFF9800),
  error('✗', 0xFFEF5350);

  final String symbol;
  final int color;

  const PeridotLogLevel(this.symbol, this.color);
}

class PeridotLogEntry {
  final DateTime time;
  final PeridotLogLevel level;
  final String source;
  final String message;

  const PeridotLogEntry({
    required this.time,
    required this.level,
    required this.source,
    required this.message,
  });
}

/// Max entries before oldest are dropped.
const int kPeridotLogMax = 200;

/// Singleton logger. Call via `PeridotLogger().info(...)` / `PeridotLogger().clear()`.
class PeridotLogger {
  PeridotLogger._();
  static final PeridotLogger _instance = PeridotLogger._();
  factory PeridotLogger() => _instance;

  final List<PeridotLogEntry> _entries = [];

  /// Snapshot of entries at call time.
  List<PeridotLogEntry> get entries =>
      List<PeridotLogEntry>.unmodifiable(_entries);

  void _add(PeridotLogLevel level, String source, String message) {
    if (_entries.length >= kPeridotLogMax) _entries.removeAt(0);
    _entries.add(PeridotLogEntry(
      time: DateTime.now(),
      level: level,
      source: source,
      message: message,
    ));
    debugPrint('[Peridot/$source] ${level.name.toUpperCase()}: $message');
  }

  void info(String source, String message) => _add(PeridotLogLevel.info, source, message);
  void ok(String source, String message) => _add(PeridotLogLevel.ok, source, message);
  void warn(String source, String message) => _add(PeridotLogLevel.warn, source, message);
  void error(String source, String message) => _add(PeridotLogLevel.error, source, message);

  /// Wrap a backend bool result.
  bool logResult(String source, String action, bool ok) {
    if (ok) {
      _add(PeridotLogLevel.ok, source, action);
    } else {
      _add(PeridotLogLevel.error, source, '$action FAILED');
    }
    return ok;
  }

  /// Wrap a nullable result — null counts as failure.
  T? logResultNull<T>(String source, String action, T? result) {
    if (result != null) {
      _add(PeridotLogLevel.ok, source, action);
    } else {
      _add(PeridotLogLevel.error, source, '$action FAILED');
    }
    return result;
  }

  void clear() => _entries.clear();
}

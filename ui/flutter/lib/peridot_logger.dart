// Peridot logger — in-memory ring buffer, accessible in-app.
//
// Skipped: disk persistence, log export (add when user requests).
// Add GET logs from daemon when needed (daemon can contribute via extended value).

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

  String get _time =>
      '${time.hour.toString().padLeft(2, '0')}:'
      '${time.minute.toString().padLeft(2, '0')}:'
      '${time.second.toString().padLeft(2, '0')}';
}

/// Max entries before oldest are dropped.
const int kPeridotLogMax = 200;

class _PeridotLogger {
  _PeridotLogger._();
  static final _PeridotLogger _instance = _PeridotLogger._();
  factory PeridotLogger() => _instance;

  final List<PeridotLogEntry> _entries = [];
  final _lock = Object();

  List<PeridotLogEntry> get entries {
    synchronized(_lock) => List.unmodifiable(_entries);
  }

  void _add(PeridotLogLevel level, String source, String message) {
    synchronized(_lock) {
      if (_entries.length >= kPeridotLogMax) _entries.removeAt(0);
      _entries.add(PeridotLogEntry(
        time: DateTime.now(),
        level: level,
        source: source,
        message: message,
      ));
    }
    debugPrint('[Peridot/${source}] ${level.name.toUpperCase()}: $message');
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

  void clear() {
    synchronized(_lock) => _entries.clear();
  }
}

/// Convenience top-level accessor.
PeridotLogEntry? Function<T>(String, String, T?) logResultNull = PeridotLogger().logResultNull;
import 'package:flutter_test/flutter_test.dart';
import 'package:peridot_essential_ui/backend/backend_connection.dart';

void main() {
  test('paused controls are Disabled, not Live or a fake Offline', () {
    expect(
      PeridotConnectionState.online.serviceLabel(enabled: false),
      'Disabled',
    );
    expect(PeridotConnectionState.online.serviceLabel(enabled: true), 'Live');
    expect(
      PeridotConnectionState.offline.serviceLabel(enabled: false),
      'Offline',
    );
    expect(
      PeridotConnectionState.connecting.serviceLabel(enabled: false),
      'Connecting',
    );
  });
  test('only a successful handshake is Live', () {
    expect(PeridotConnectionState.fromNative(1), PeridotConnectionState.online);
    expect(PeridotConnectionState.fromNative(1).badgeLabel, 'LIVE');
  });

  test('startup and a confirmed connection failure remain distinct', () {
    expect(
      PeridotConnectionState.fromNative(-1),
      PeridotConnectionState.connecting,
    );
    expect(PeridotConnectionState.fromNative(-1).badgeLabel, 'CONNECTING');
    expect(PeridotConnectionState.fromNative(0), PeridotConnectionState.offline);
    expect(PeridotConnectionState.fromNative(0).badgeLabel, 'OFFLINE');
  });
}

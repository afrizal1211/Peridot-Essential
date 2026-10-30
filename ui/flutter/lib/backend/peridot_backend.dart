import 'dart:async';
import 'dart:ffi' as ffi;
import 'dart:typed_data';

import '../peridot_logger.dart';
import 'backend_connection.dart';
import 'system_colors_state.dart';
import 'system_colors_preferences.dart';
import 'system_colors_monitor.dart';
export 'backend_connection.dart';
export 'system_colors_state.dart';

typedef _BackendGetNative = ffi.Int32 Function(ffi.Int32);
typedef _BackendGetDart = int Function(int);
typedef _BackendSetNative = ffi.Int32 Function(ffi.Int32, ffi.Int32, ffi.Int32);
typedef _BackendSetDart = int Function(int, int, int);
typedef _BackEventNative = ffi.Void Function(ffi.Int32);
typedef _BackListenerNative = ffi.Void Function(
  ffi.Pointer<ffi.NativeFunction<_BackEventNative>>,
);
typedef _BackListenerDart = void Function(
  ffi.Pointer<ffi.NativeFunction<_BackEventNative>>,
);

final class PeridotBackendSnapshot {
  const PeridotBackendSnapshot({
    this.serviceEnabled = true,
    required this.connection,
    required this.actionState,
    required this.chargingWriteState,
    required this.chargingMode,
    required this.batteryCapacity,
    required this.batteryTempTenthsC,
    required this.batteryVoltageUv,
    required this.batteryCurrentUa,
    required this.batteryStatus,
    required this.batteryHealth,
    required this.usbOnline,
    required this.usbType,
    required this.cpuOnlineMask,
    required this.cpuFreqKhz,
    required this.cpuGovernor0,
    required this.cpuGovernor3,
    required this.cpuGovernor7,
    required this.gpuGovernor,
    required this.ufsScheduler,
    required this.touchHal,
    required this.displayHal,
    required this.touchState,
    required this.displayColor,
    required this.displayTemp,
    required this.sunlight,
    required this.silky,
    required this.video,
    required this.dolby,
    required this.performanceProfile,
  });

  final PeridotConnectionState connection;
  final bool serviceEnabled;
  bool get controlsAvailable => ready && serviceEnabled;
  String get serviceStatusLabel =>
      connection.serviceLabel(enabled: serviceEnabled);
  String get serviceStatusBadgeLabel => serviceStatusLabel.toUpperCase();
  bool get ready => connection == PeridotConnectionState.online;
  final int actionState;
  final int chargingWriteState;
  final int chargingMode;
  final int batteryCapacity;
  final int batteryTempTenthsC;
  final int batteryVoltageUv;
  final int batteryCurrentUa;
  final int batteryStatus;
  final int batteryHealth;
  final int usbOnline;
  final int usbType;
  final int cpuOnlineMask;
  final List<int> cpuFreqKhz;
  final int cpuGovernor0;
  final int cpuGovernor3;
  final int cpuGovernor7;
  final int gpuGovernor;
  final int ufsScheduler;
  final int touchHal;
  final int displayHal;
  final int touchState;
  final int displayColor;
  final int displayTemp;
  final int sunlight;
  final int silky;
  final int video;
  final int dolby;
  final int performanceProfile;

  bool get chargingBoost => chargingMode == 90;
  bool get busy => actionState == 1;
  double? get batteryTempC =>
      batteryTempTenthsC >= 0 ? batteryTempTenthsC / 10.0 : null;
  double? get batteryVoltageV =>
      batteryVoltageUv >= 0 ? batteryVoltageUv / 1000000.0 : null;
  double? get batteryCurrentA => batteryCurrentUa != -9223372036854775808
      ? batteryCurrentUa / 1000000.0
      : null;

  bool cpuOnline(int cpu) =>
      cpu >= 0 && cpu < 32 && (cpuOnlineMask & (1 << cpu)) != 0;
}

typedef _I32Native = ffi.Int32 Function();
typedef _I32Dart = int Function();
typedef _I64Native = ffi.Int64 Function();
typedef _I64Dart = int Function();
typedef _U32Native = ffi.Uint32 Function();
typedef _U32Dart = int Function();
typedef _U32ToI64Native = ffi.Int64 Function(ffi.Uint32);
typedef _U32ToI64Dart = int Function(int);
typedef _SetI32Native = ffi.Int32 Function(ffi.Int32);
typedef _SetI32Dart = int Function(int);
typedef _I32ToI32Native = ffi.Int32 Function(ffi.Int32);
typedef _I32ToI32Dart = int Function(int);
typedef _SetTwoI32Native = ffi.Int32 Function(ffi.Int32, ffi.Int32);
typedef _SetTwoI32Dart = int Function(int, int);
typedef _GetLogsNative = ffi.Int32 Function(
    ffi.Pointer<ffi.Uint8>, ffi.Int32);
typedef _GetLogsDart = int Function(ffi.Pointer<ffi.Uint8>, int);
typedef _LogsAllocNative = ffi.Pointer<ffi.Void> Function(ffi.IntPtr);
typedef _LogsAllocDart = ffi.Pointer<ffi.Void> Function(int);
typedef _LogsFreeNative = ffi.Void Function(ffi.Pointer<ffi.Void>);
typedef _LogsFreeDart = void Function(ffi.Pointer<ffi.Void>);

final class PeridotBackend {
  PeridotBackend._();
  static final PeridotBackend instance = PeridotBackend._();

  ffi.DynamicLibrary? _lib;
  Timer? _timer;
  PeridotSystemColorsMonitor? _paletteMonitor;
  PeridotSystemColorsMonitor? _contrastMonitor;
  bool _started = false;

  final StreamController<PeridotBackendSnapshot> _controller =
      StreamController<PeridotBackendSnapshot>.broadcast(sync: true);
  final Map<int, List<int>> _cpuFrequencyTables = <int, List<int>>{};

  late _I32Dart _startNative;
  late _I32Dart _refreshNative;
  late _I32Dart _readyNative;
  late _I32Dart _actionStateNative;
  late _I32Dart _chargingWriteStateNative;
  late _I32Dart _chargingModeNative;
  late _SetI32Dart _setChargingModeNative;
  late _SetI32Dart _setBypassChargingNative;
  late _I32Dart _batteryCapacityNative;
  late _I32Dart _batteryTempNative;
  late _I64Dart _batteryVoltageNative;
  late _I64Dart _batteryCurrentNative;
  late _I32Dart _batteryStatusNative;
  late _I32Dart _batteryHealthNative;
  late _I32Dart _usbOnlineNative;
  late _I32Dart _usbTypeNative;
  late _U32Dart _cpuOnlineMaskNative;
  late _U32ToI64Dart _cpuFreqNative;
  late _I32ToI32Dart _cpuGovernorNative;
  late _SetTwoI32Dart _setCpuGovernorNative;
  late _I32ToI32Dart _cpuAvailableCountNative;
  late _SetTwoI32Dart _cpuAvailableFreqNative;
  late _GetLogsDart _logsNative;
  late _LogsAllocDart _mallocNative;
  late _LogsFreeDart _freeNative;
  late _I32Dart _gpuGovernorNative;
  late _SetI32Dart _setGpuGovernorNative;
  late _I32Dart _ufsSchedulerNative;
  late _SetI32Dart _setUfsSchedulerNative;
  late _I32Dart _touchHalNative;
  late _I32Dart _displayHalNative;
  late _I32Dart _touchStateNative;
  late _SetI32Dart _setTouchStateNative;
  late _I32Dart _displayColorNative;
  late _SetI32Dart _setDisplayColorNative;
  late _I32Dart _displayTempNative;
  late _SetI32Dart _setDisplayTempNative;
  late _I32Dart _sunlightNative;
  late _SetI32Dart _setSunlightNative;
  late _I32Dart _silkyNative;
  late _SetI32Dart _setSilkyNative;
  late _I32Dart _videoNative;
  late _SetI32Dart _setVideoNative;
  late _I32Dart _dolbyNative;
  late _SetI32Dart _setDolbyNative;
  late _I32Dart _performanceNative;
  late _SetI32Dart _setPerformanceNative;
  late _SetI32Dart _openSupportLinkNative;
  late _I32Dart _photoPermissionStateNative;
  late _I32Dart _requestPhotoPermissionNative;
  late _SetI32Dart _hapticNative;
  late _BackendGetDart _backendGetNative;
  late _BackendSetDart _backendSetNative;
  late _SetI32Dart _setBackInterceptNative;
  late _I32Dart _consumeBackRequestNative;
  _I32Dart? _consumeBackGestureNative;
  _I32Dart? _consumeBypassSettingsNative;
  _BackListenerDart? _setBackListenerNative;
  ffi.NativeCallable<_BackEventNative>? _backListener;

  PeridotBackendSnapshot _latest = const PeridotBackendSnapshot(
    connection: PeridotConnectionState.connecting,
    actionState: 0,
    chargingWriteState: 0,
    chargingMode: -1,
    batteryCapacity: -1,
    batteryTempTenthsC: -1,
    batteryVoltageUv: -1,
    batteryCurrentUa: -9223372036854775808,
    batteryStatus: -1,
    batteryHealth: -1,
    usbOnline: -1,
    usbType: -1,
    cpuOnlineMask: 0,
    cpuFreqKhz: <int>[-1, -1, -1, -1, -1, -1, -1, -1],
    cpuGovernor0: -1,
    cpuGovernor3: -1,
    cpuGovernor7: -1,
    gpuGovernor: -1,
    ufsScheduler: -1,
    touchHal: 0,
    displayHal: 0,
    touchState: -1,
    displayColor: -1,
    displayTemp: -1,
    sunlight: -1,
    silky: -1,
    video: -1,
    dolby: -1,
    performanceProfile: -1,
  );

  PeridotBackendSnapshot get latest => _latest;
  Stream<PeridotBackendSnapshot> get snapshots => _controller.stream;
  bool get started => _started;

  PeridotSystemColorsSelection? _systemColorsSelection;
  PeridotSystemColorsSelection? get systemColorsSelection =>
      _systemColorsSelection;
  int _systemColorsRevision = 0;
  Future<void> _systemColorsSave = Future<void>.value();

  Future<void> _loadSystemColorsSelection() async {
    final PeridotSystemColorsSelection? selection =
        await PeridotSystemColorsPreferences.read();
    // An actual readback that finished meanwhile always wins over the hint.
    if (_systemColorsRevision == 0 && selection != _systemColorsSelection) {
      _systemColorsSelection = selection;
      _controller.add(_latest);
    }
  }

  void _rememberSystemColors() {
    final PeridotSystemColorsState palette = systemColors;
    if (!palette.ready ||
        !palette.hasReadback ||
        palette.revision == _systemColorsRevision)
      return;
    final bool firstRead = _systemColorsRevision == 0;
    _systemColorsRevision = palette.revision;
    final PeridotSystemColorsSelection? selection =
        PeridotSystemColorsSelection.fromNative(palette);
    if (selection == _systemColorsSelection && !firstRead) return;
    _systemColorsSelection = selection;
    // Serialize these small writes so an older completion cannot replace a
    // newer confirmed selection. This is not a background system reapply.
    _systemColorsSave = _systemColorsSave.then(
      (_) => PeridotSystemColorsPreferences.write(selection),
    );
  }

  bool start() {
    if (_started) return true;
    try {
      final ffi.DynamicLibrary lib = ffi.DynamicLibrary.open(
        'libperidot_essential_host.so',
      );
      _startNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_start',
      );
      _refreshNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_refresh',
      );
      _readyNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_ready',
      );
      _actionStateNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_action_state',
      );
      _chargingWriteStateNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_charging_write_state',
      );
      _chargingModeNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_charging_mode',
      );
      _setChargingModeNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_charging_mode',
      );
      _setBypassChargingNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_bypass_charging',
      );
      _batteryCapacityNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_battery_capacity',
      );
      _batteryTempNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_battery_temp_tenths_c',
      );
      _batteryVoltageNative = lib.lookupFunction<_I64Native, _I64Dart>(
        'peridot_backend_get_battery_voltage_uv',
      );
      _batteryCurrentNative = lib.lookupFunction<_I64Native, _I64Dart>(
        'peridot_backend_get_battery_current_ua',
      );
      _batteryStatusNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_battery_status',
      );
      _batteryHealthNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_battery_health',
      );
      _usbOnlineNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_usb_online',
      );
      _usbTypeNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_usb_type',
      );
      _cpuOnlineMaskNative = lib.lookupFunction<_U32Native, _U32Dart>(
        'peridot_backend_get_cpu_online_mask',
      );
      _cpuFreqNative = lib.lookupFunction<_U32ToI64Native, _U32ToI64Dart>(
        'peridot_backend_get_cpu_freq_khz',
      );
      _cpuGovernorNative = lib.lookupFunction<_I32ToI32Native, _I32ToI32Dart>(
        'peridot_backend_get_cpu_governor',
      );
      _setCpuGovernorNative = lib
          .lookupFunction<_SetTwoI32Native, _SetTwoI32Dart>(
            'peridot_backend_set_cpu_governor',
          );
      _cpuAvailableCountNative = lib
          .lookupFunction<_I32ToI32Native, _I32ToI32Dart>(
            'peridot_backend_get_cpu_available_count',
          );
      _cpuAvailableFreqNative = lib
          .lookupFunction<_SetTwoI32Native, _SetTwoI32Dart>(
            'peridot_backend_get_cpu_available_freq',
          );
      _gpuGovernorNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_gpu_governor',
      );
      _setGpuGovernorNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_gpu_governor',
      );
      _ufsSchedulerNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_ufs_scheduler',
      );
      _setUfsSchedulerNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_ufs_scheduler',
      );
      _touchHalNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_touch_hal',
      );
      _displayHalNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_display_hal',
      );
      _touchStateNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_touch_state',
      );
      _setTouchStateNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_touch_state',
      );
      _displayColorNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_display_color',
      );
      _setDisplayColorNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_display_color',
      );
      _displayTempNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_display_temp',
      );
      _setDisplayTempNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_display_temp',
      );
      _sunlightNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_sunlight',
      );
      _setSunlightNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_sunlight',
      );
      _silkyNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_silky',
      );
      _setSilkyNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_silky',
      );
      _videoNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_video',
      );
      _setVideoNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_video',
      );
      _dolbyNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_dolby',
      );
      _setDolbyNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_dolby',
      );
      _performanceNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_backend_get_performance_profile',
      );
      _setPerformanceNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_set_performance_profile',
      );
      _openSupportLinkNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_backend_open_support_link',
      );
      _hapticNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_host_haptic',
      );
      _backendGetNative = lib
          .lookupFunction<_BackendGetNative, _BackendGetDart>(
            'peridot_backend_extended_get',
          );
      _backendSetNative = lib
          .lookupFunction<_BackendSetNative, _BackendSetDart>(
            'peridot_backend_extended_set',
          );

      _setBackInterceptNative = lib.lookupFunction<_SetI32Native, _SetI32Dart>(
        'peridot_host_set_back_intercept',
      );
      _consumeBackRequestNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_host_consume_back_request',
      );
      if (lib.providesSymbol('peridot_host_consume_bypass_settings_request')) {
        _consumeBypassSettingsNative = lib.lookupFunction<_I32Native, _I32Dart>(
          'peridot_host_consume_bypass_settings_request',
        );
      }
      if (lib.providesSymbol('peridot_host_consume_back_gesture')) {
        _consumeBackGestureNative = lib.lookupFunction<_I32Native, _I32Dart>(
          'peridot_host_consume_back_gesture',
        );
      }
      if (lib.providesSymbol('peridot_host_set_back_listener')) {
        _setBackListenerNative = lib
            .lookupFunction<_BackListenerNative, _BackListenerDart>(
              'peridot_host_set_back_listener',
            );
      }

      _photoPermissionStateNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_host_photo_permission_state',
      );
      _requestPhotoPermissionNative = lib.lookupFunction<_I32Native, _I32Dart>(
        'peridot_host_request_photo_permission',
      );

      final ffi.DynamicLibrary libc = ffi.DynamicLibrary.open('libc.so');
      _mallocNative = libc.lookupFunction<_LogsAllocNative, _LogsAllocDart>('malloc');
      _freeNative = libc.lookupFunction<_LogsFreeNative, _LogsFreeDart>('free');
      _logsNative = lib.lookupFunction<_GetLogsNative, _GetLogsDart>(
        'peridot_backend_get_logs',
      );

      _lib = lib;
      _started = _startNative() == 1;
      if (_started) {
        unawaited(_loadSystemColorsSelection());
        _refreshNative();
        _poll();
        _timer = Timer.periodic(
          const Duration(milliseconds: 500),
          (_) => _poll(),
        );
      }
      return _started;
    } catch (_) {
      _started = false;
      return false;
    }
  }

  void _poll() {
    if (!_started || _lib == null) return;
    _rememberSystemColors();
    final PeridotBackendSnapshot next = PeridotBackendSnapshot(
      serviceEnabled: _backendGetNative(116) != 0,
      connection: PeridotConnectionState.fromNative(_readyNative()),
      actionState: _actionStateNative(),
      chargingWriteState: _chargingWriteStateNative(),
      chargingMode: _chargingModeNative(),
      batteryCapacity: _batteryCapacityNative(),
      batteryTempTenthsC: _batteryTempNative(),
      batteryVoltageUv: _batteryVoltageNative(),
      batteryCurrentUa: _batteryCurrentNative(),
      batteryStatus: _batteryStatusNative(),
      batteryHealth: _batteryHealthNative(),
      usbOnline: _usbOnlineNative(),
      usbType: _usbTypeNative(),
      cpuOnlineMask: _cpuOnlineMaskNative(),
      cpuFreqKhz: List<int>.generate(8, _cpuFreqNative, growable: false),
      cpuGovernor0: _cpuGovernorNative(0),
      cpuGovernor3: _cpuGovernorNative(3),
      cpuGovernor7: _cpuGovernorNative(7),
      gpuGovernor: _gpuGovernorNative(),
      ufsScheduler: _ufsSchedulerNative(),
      touchHal: _touchHalNative(),
      displayHal: _displayHalNative(),
      touchState: _touchStateNative(),
      displayColor: _displayColorNative(),
      displayTemp: _displayTempNative(),
      sunlight: _sunlightNative(),
      silky: _silkyNative(),
      video: _videoNative(),
      dolby: _dolbyNative(),
      performanceProfile: _performanceNative(),
    );
    _latest = next;
    _controller.add(next);
  }

  bool _queue(int rc, String label) {
    final bool accepted = _started && rc == 1;
    if (accepted) {
      PeridotLogger().info('backend', label);
    } else if (_started) {
      PeridotLogger().error('backend', '$label queued FAILED');
    }
    if (!accepted) return false;

    Timer(const Duration(milliseconds: 24), _poll);
    Timer(const Duration(milliseconds: 64), _poll);
    Timer(const Duration(milliseconds: 120), _poll);
    Timer(const Duration(milliseconds: 220), _poll);

    return true;
  }

  bool setChargingProfile(int watts) {
    if (!const <int>{0, 25, 33, 65, 85, 90}.contains(watts)) return false;
    return _queue(_setChargingModeNative(watts), 'set charging profile $watts');
  }

  bool setChargingBoost(bool enabled) => setChargingProfile(enabled ? 90 : 0);

  bool get chargingControlSupported => extendedValue(105) == 1;
  int get chargingMode {
    // fastcharge_enable 0=none, 1=fast, 2=super fast. Map to the UI profile
    // scale (0/25/33/65/85/90) so existing labels and selection tiles work.
    switch (extendedValue(106)) {
      case 2:
        return 90;
      case 1:
        return 33;
      default:
        return 0;
    }
  }
  int get chargingFastcharge => extendedValue(106);
  int get chargingAdapterWatts => extendedValue(107);
  bool get chargingPdAuthenticated => extendedValue(108) == 1;
  int get chargingQuickType => extendedValue(109);
  int get chargingSicMode => extendedValue(110);
  int get chargingLiveMilliwatts => extendedValue(111);
  bool get bypassChargingSupported => extendedValue(112) == 1;
  int get bypassChargingState => extendedValue(113);
  bool get bypassChargingEnabled => bypassChargingSaved == 1;
  int get bypassChargingSaved => extendedValue(114);
  bool get bypassHardwareActive => extendedValue(115) == 1;
  int get bypassThreshold => extendedValue(119);
  int get bypassPhase => extendedValue(120);
  bool setBypassThreshold(int percent) =>
      const <int>{0, 20, 40, 80, 90}.contains(percent) &&
      setExtendedOperation(31, percent);
  bool setBypassCharging(bool enabled) =>
      _queue(_setBypassChargingNative(enabled ? 1 : 0), 'set bypass charging $enabled');
  bool setTouchProfile(int profile) {
    if (profile != 0 && profile != 1) return false;
    return _queue(_setTouchStateNative(profile), 'set touch profile $profile');
  }

  // Kept for older call sites outside the Flutter screen bundle. Enabled maps
  // to Xiaomi game mode; disabled selects OEM (game mode off).
  bool setTouchBoost(bool enabled) => setTouchProfile(enabled ? 1 : 0);
  bool setDisplayColor(int mode) => _queue(_setDisplayColorNative(mode), 'set display color $mode');
  bool setDisplayTemperature(int mode) => _queue(_setDisplayTempNative(mode), 'set display temp $mode');
  bool setSunlight(bool enabled) => _queue(_setSunlightNative(enabled ? 1 : 0), 'set sunlight $enabled');
  bool setSilky(bool enabled) => _queue(_setSilkyNative(enabled ? 1 : 0), 'set silky $enabled');
  bool setVideoEnhancement(bool enabled) =>
      _queue(_setVideoNative(enabled ? 1 : 0), 'set video $enabled');
  bool setDolbyVision(bool enabled) => _queue(_setDolbyNative(enabled ? 1 : 0), 'set dolby $enabled');
  bool setPerformanceProfile(int profile) {
    if (_latest.performanceProfile != profile) {
      _latest = PeridotBackendSnapshot(
        connection: _latest.connection,
        serviceEnabled: _latest.serviceEnabled,
        actionState: _latest.actionState,
        chargingWriteState: _latest.chargingWriteState,
        chargingMode: _latest.chargingMode,
        batteryCapacity: _latest.batteryCapacity,
        batteryTempTenthsC: _latest.batteryTempTenthsC,
        batteryVoltageUv: _latest.batteryVoltageUv,
        batteryCurrentUa: _latest.batteryCurrentUa,
        batteryStatus: _latest.batteryStatus,
        batteryHealth: _latest.batteryHealth,
        usbOnline: _latest.usbOnline,
        usbType: _latest.usbType,
        cpuOnlineMask: _latest.cpuOnlineMask,
        cpuFreqKhz: _latest.cpuFreqKhz,
        cpuGovernor0: _latest.cpuGovernor0,
        cpuGovernor3: _latest.cpuGovernor3,
        cpuGovernor7: _latest.cpuGovernor7,
        gpuGovernor: switch (profile) {
          0 => 0, // OEM dummy
          1 => 3, // simple_ondemand
          2 => 1, // powersave
          3 => 2, // performance
          _ => _latest.gpuGovernor,
        },
        ufsScheduler: _latest.ufsScheduler,
        touchHal: _latest.touchHal,
        displayHal: _latest.displayHal,
        touchState: _latest.touchState,
        displayColor: _latest.displayColor,
        displayTemp: _latest.displayTemp,
        sunlight: _latest.sunlight,
        silky: _latest.silky,
        video: _latest.video,
        dolby: _latest.dolby,
        performanceProfile: profile,
      );
      if (!_controller.isClosed) {
        _controller.add(_latest);
      }
    }
    return _queue(_setPerformanceNative(profile), 'set perf profile $profile');
  }

  bool setCpuGovernor(int policy, int code) =>
      _queue(_setCpuGovernorNative(policy, code), 'set cpu$policy governor');
  bool setGpuGovernor(int code) => _queue(_setGpuGovernorNative(code), 'set gpu governor');
  bool setUfsScheduler(int code) => _queue(_setUfsSchedulerNative(code), 'set ufs scheduler');
  bool openSupportLink(int code) {
    if (!_started) return false;
    return _openSupportLinkNative(code) == 1;
  }

  bool setBackIntercept(bool enabled) {
    if (!_started) return false;
    return _setBackInterceptNative(enabled ? 1 : 0) == 1;
  }

  bool consumeBackRequest() {
    if (!_started) return false;
    return _consumeBackRequestNative() == 1;
  }

  int consumeBackGesture() => _consumeBackGestureNative?.call() ?? 0;
  bool consumeBypassSettingsRequest() =>
      _consumeBypassSettingsNative?.call() == 1;

  bool listenBackGesture(void Function(int) onEvent) {
    stopBackGesture();
    if (!_started || _setBackListenerNative == null) return false;
    final callback = ffi.NativeCallable<_BackEventNative>.listener(onEvent);
    _backListener = callback;
    _setBackListenerNative!(callback.nativeFunction);
    return true;
  }

  void stopBackGesture() {
    _setBackListenerNative?.call(ffi.nullptr);
    _backListener?.close();
    _backListener = null;
  }

  int extendedValue(int key) {
    if (!_started || _lib == null) return -1;
    return _backendGetNative(key);
  }

  bool setExtendedOperation(int op, int a, [int b = 0]) {
    if (!_started || _lib == null) return false;
    final String opLabel = switch (op) {
      18 => 'cpu core online a=$a',
      19 => 'cpu${(a)} min freq',
      20 => 'cpu${(a)} max freq',
      21 => 'cpu${(a)} range',
      22 => 'gpu power policy',
      23 => 'cpu${(a)} freq reset',
      31 => 'bypass threshold $a',
      _ => 'op $op a=$a b=$b',
    };
    final bool res = _queue(_backendSetNative(op, a, b), 'set $opLabel');
    refresh();
    return res;
  }

  PeridotSystemColorsState get systemColors => _started
      ? PeridotSystemColorsState.fromNative(extendedValue)
      : const PeridotSystemColorsState();

  // Palette transactions have their own result and revision in the native
  // cache. Do not trigger hardware refresh/reapply operations from this screen.
  bool _queueSystemColors(int operation, [int seed = 0, int style = 0]) {
    if (!_started || _lib == null) return false;
    if (_backendSetNative(operation, seed, style) != 1) return false;
    // The normal dashboard cadence is 500 ms. Palette completions must not
    // sit behind it while the user is choosing the next color. Read only two
    // atomic cache fields until completion, then stop completely.
    (_paletteMonitor ??= PeridotSystemColorsMonitor(
      readStamp: () => (extendedValue(81), extendedValue(94)),
      onChanged: () {
        _rememberSystemColors();
        _controller.add(_latest);
      },
    )).watch();
    return true;
  }

  bool refreshSystemColors() => _queueSystemColors(26);

  bool refreshSystemColorContrast() => _queueSystemColorContrast(27);

  bool _queueSystemColorContrast(int operation, [int level = 0]) {
    if (!_started || _lib == null) return false;
    if (_backendSetNative(operation, level, 0) != 1) return false;
    (_contrastMonitor ??= PeridotSystemColorsMonitor(
      readStamp: () => (extendedValue(96), extendedValue(104)),
      onChanged: () => _controller.add(_latest),
    )).watch();
    return true;
  }

  bool setSystemColorContrast(int level) {
    if (level < -1000 || level > 1000) return false;
    return _queueSystemColorContrast(28, level);
  }

  bool useWallpaperSystemColors() => _queueSystemColors(25);

  bool setSystemColors(int seed, int style) {
    if (seed < 0 || seed > 0xffffff || style < 0 || style > 6) return false;
    return _queueSystemColors(24, seed, style);
  }

  bool setExpertGamut(int gamut) => setExtendedOperation(2, gamut);

  bool setExpertChannel(int channel, int value) =>
      setExtendedOperation(3, channel, value);

  bool resetExpertDisplay() => setExtendedOperation(4, 0);

  bool get serviceEnabled => extendedValue(116) == 1;
  int get serviceActionState => extendedValue(118);
  bool setServiceEnabled(bool enabled) =>
      setExtendedOperation(29, enabled ? 1 : 0);
  bool resetAllSettings() => setExtendedOperation(30, 0);

  bool setCpuManualMode(bool enabled) =>
      setExtendedOperation(7, enabled ? 1 : 0);

  bool setCpuCoreOnline(int cpu, bool online) {
    if (cpu < 1 || cpu > 7) return false;
    return setExtendedOperation(8, cpu, online ? 1 : 0);
  }

  bool setDisplayResolution(int width, int height, [int density = 0]) {
    if (width <= 0 || height <= 0 || (width == 1220 && height == 2712)) {
      return resetDisplayResolution();
    }
    final int detectedDensity = extendedValue(78);
    final int nativeDensity = detectedDensity > 0 ? detectedDensity : 520;
    final int safeDensity = density > 0
        ? density
        : ((nativeDensity * width + 610) ~/ 1220);
    return setExtendedOperation(
      9,
      width,
      (height << 16) | (safeDensity & 0xFFFF),
    );
  }

  bool resetDisplayResolution() => setExtendedOperation(9, 0, 0);

  bool setZramSize(int sizeMb) => setExtendedOperation(10, sizeMb);

  bool setZramAlgorithm(int algCode) => setExtendedOperation(11, algCode);

  bool setZramSwappiness(int swappiness) =>
      setExtendedOperation(12, swappiness);

  bool compactZram() => setExtendedOperation(13, 0);

  bool setGpuUncap(bool uncap) => setExtendedOperation(14, uncap ? 1 : 0);

  bool setGpuMinFreq(int mhz) => setExtendedOperation(15, mhz);

  bool setGpuMaxFreq(int mhz) => setExtendedOperation(16, mhz);

  bool setGpuDevfreqGovernor(int code) => setExtendedOperation(17, code);

  bool setGpuGedBoost(bool boost) => setExtendedOperation(18, boost ? 1 : 0);

  bool setCpuClusterMinFreq(int policy, int mhz) =>
      setExtendedOperation(19, policy, mhz);

  bool setCpuClusterMaxFreq(int policy, int mhz) =>
      setExtendedOperation(20, policy, mhz);

  bool setCpuClusterFreqRange(int policy, int minMhz, int maxMhz) {
    return setExtendedOperation(21, policy, (minMhz << 16) | (maxMhz & 0xFFFF));
  }

  bool resetCpuClusterFreqRange(int policy) => setExtendedOperation(23, policy);

  List<int> cpuAvailableFrequencies(int policy) {
    if (!_started ||
        _lib == null ||
        !const <int>{0, 3, 7}.contains(policy)) {
      return const <int>[];
    }

    final List<int>? cached = _cpuFrequencyTables[policy];
    if (cached != null && cached.isNotEmpty) return cached;

    final int count = _cpuAvailableCountNative(policy).clamp(0, 128).toInt();
    final List<int> frequencies = <int>[];
    for (int index = 0; index < count; index++) {
      final int mhz = _cpuAvailableFreqNative(policy, index);
      if (mhz > 0) frequencies.add(mhz);
    }
    final List<int> immutable = List<int>.unmodifiable(frequencies);
    if (immutable.isNotEmpty) _cpuFrequencyTables[policy] = immutable;
    return immutable;
  }

  int cpuTargetMinFrequency(int policy) => switch (policy) {
    0 => extendedValue(53),
    3 => extendedValue(55),
    7 => extendedValue(57),
    _ => -1,
  };

  int cpuTargetMaxFrequency(int policy) => switch (policy) {
    0 => extendedValue(54),
    3 => extendedValue(56),
    7 => extendedValue(58),
    _ => -1,
  };

  int cpuLiveMinFrequency(int policy) => switch (policy) {
    0 => extendedValue(68),
    3 => extendedValue(70),
    7 => extendedValue(72),
    _ => -1,
  };

  int cpuLiveMaxFrequency(int policy) => switch (policy) {
    0 => extendedValue(69),
    3 => extendedValue(71),
    7 => extendedValue(73),
    _ => -1,
  };

  int cpuFrequencyDrift(int policy) => switch (policy) {
    0 => extendedValue(75),
    3 => extendedValue(76),
    7 => extendedValue(77),
    _ => -1,
  };

  bool setGpuPowerPolicy(int policyCode) =>
      setExtendedOperation(22, policyCode);

  int photoPermissionState() {
    if (!_started) return 0;
    return _photoPermissionStateNative();
  }

  bool requestPhotoPermission() {
    if (!_started) return false;
    return _requestPhotoPermissionNative() == 1;
  }

  /// Fetch daemon + host log entries for the in-app log viewer.
  /// Returns pipe-separated "timestamp source LEVEL message" entries.
  /// Skipped: error handling for malloc failures (returns empty on OOM).
  List<String> fetchLogs() {
    if (!_started) return [];
    // 64 KB buffer — sufficient for 100 daemon entries (~250 bytes each) plus
    // 100 host entries (~120 bytes each) = ~37 KB. Falls back to empty on
    // symbol-not-found or alloc failure.
    const int cap = 65536;
    final ffi.Pointer<ffi.Void> buf = _mallocNative(cap);
    if (buf == ffi.nullptr) return [];
    try {
      final ffi.Pointer<ffi.Uint8> out = buf.cast<ffi.Uint8>();
      final int len = _logsNative(out, cap);
      if (len <= 0 || len > cap) return [];
      // len bytes written at buf — decode UTF-8.
      final Uint8List view = out.asTypedList(len);
      final String text = String.fromCharCodes(view);
      return text.isEmpty ? [] : text.split('|');
    } catch (_) {
      return [];
    } finally {
      _freeNative(buf);
    }
  }

  void refresh() {
    if (_started) _refreshNative();
  }

  void dispose() {
    stopBackGesture();
    _paletteMonitor?.dispose();
    _contrastMonitor?.dispose();
    _timer?.cancel();
    _timer = null;
  }

  bool haptic(int kind) {
    if (!_started || kind < 1 || kind > 9) {
      return false;
    }

    return _hapticNative(kind) == 1;
  }
}

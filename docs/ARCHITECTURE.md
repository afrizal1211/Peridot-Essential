# Peridot architecture and diagnostics

The Android app (`io.github.davvv98.peridotessential`) loads Flutter AOT and the
Rust host. The host sends commands to the privileged `peridot_daemon`; the app
does not use the system UID. Transport authenticates the client identity.
Both ends must use protocol `peridot.5`.

## CPU

| Policy | Cores |
| --- | --- |
| 0 | 0–1 |
| 2 | 2–4 |
| 5 | 5–6 |
| 7 | 7 |

Frequency tables come from `scaling_available_frequencies`, falling back to
`stats/time_in_state`. Integer MHz UI identifiers resolve to exact kHz before
writing: for example, 3014 represents 3014400 kHz. Ambiguous resolutions are
rejected. A frequency is not invented just because it appears in cpuinfo_max_freq.

`cpu_minN/maxN` contain requests; `cpu_live_minN/maxN` contain effective bounds.
`cpu_freq_driftN=0` means a match; `1` means a difference. `cpu_freq_ack` describes
the last submission: 1 matched at readback, 2 was accepted with different bounds,
0 failed, and -1 has no result. Use the live fields to check subsequent changes.

## GPU

The backend uses `/sys/class/kgsl/kgsl-3d0/devfreq`. `gpu_requested_min/max`
store requests; `gpu_min/max_freq` report effective bounds and `gpu_cur_freq`
is the instantaneous frequency. `perf` selects Stock=0, Gaming=1, Saver=2,
Extreme=3. `perf_verify_ok=1` indicates matching bounds at verification time.

Stock restores the captured range. Gaming requests the full table. Saver requests
an upper OPP no higher than 510 MHz. Extreme requests minimum=maximum at the
highest OPP. Driver adjustments are distinct from denied writes. IO, permission
and invalid-read failures remain errors. Failed transactions attempt to restore
the previous bounds and report restoration failures.

## Querying the installed module

Run the installed client as root in Termux:

```sh
su -c '
for ctl in /data/adb/modules/*/bin/peridot_ctl; do
    [ ! -x "$ctl" ] || timeout 12 "$ctl" GET snapshot
done
'
```

`diagnostico_peridot_instalado.sh` adds read-only logs and node inspection.
Attempt the action in the app first to capture its result in the log.
Android logs use the `PeridotEssential` tag.

Bounds are submitted through user actions, per-app profiles and bounded startup
restoration. No loop continuously forces requests after the system changes them.

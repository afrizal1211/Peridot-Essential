# Memory and storage

Peridot retains the ZRAM, swappiness and storage discovery backends. The interface
must follow detected nodes rather than fixed paths or clocks from another platform.

Changing ZRAM size or compression may require recreating the swap device.
Do not repeat those operations when the configuration already matches.
Memory/UFS clock options require actual backend capability detection; they have
not been validated for this POCO F7 adaptation.

Check current readback and operation errors. A saved profile does not prove that
a device applied a clock or scheduler.

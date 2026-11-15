"""Regression coverage for the porter's battery-manager traversal denial."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ChargingPolicyTests(unittest.TestCase):
    def test_aosp_parent_permission_is_search_only(self):
        source = (ROOT / 'android/aosp/sepolicy/vendor/peridot_daemon.te').read_text()
        rules = [line.strip() for line in source.splitlines()
                 if line.strip().startswith('allow ') and 'sysfs_batteryinfo' in line]
        self.assertEqual(rules, ['allow peridot_daemon sysfs_batteryinfo:dir search;'])
        for label in ('sysfs_battery_supply', 'sysfs_usb_supply'):
            self.assertIn(f'allow peridot_daemon {label}:dir search;', source)
            self.assertIn(f'allow peridot_daemon {label}:file rw_file_perms;', source)

    def test_unpacked_parent_permission_matches_aosp(self):
        source = (ROOT / 'android/unpacked/peridot-native.cil').read_text()
        rules = [line.strip() for line in source.splitlines()
                 if line.strip().startswith('(allow ') and 'sysfs_batteryinfo' in line]
        self.assertEqual(rules, ['(allow peridot_daemon sysfs_batteryinfo (dir (search)))'])

    def test_rom_packager_uses_production_adapter_check(self):
        source = (ROOT / 'tools/build-rom-integration-package.py').read_text()
        self.assertIn('tools/check-platform-dex.sh', source)
        self.assertNotIn("'Unexpected DEX'", source)

    def test_ram_parent_label_is_preserved(self):
        source = (ROOT / 'android/unpacked/peridot-native.cil').read_text()
        # Upstream MediaTek policy must not survive the Peridot port.
        self.assertNotIn('dvfsrc', source)
        self.assertNotIn('mtk_hal_power', source)
        self.assertNotIn('112b0000.ufshci', source)
        # QCOM DCVS: the parent tree stays read-only metadata and each rail's
        # boost_freq floor request is the writable labeled leaf.
        self.assertIn(
            '(genfscon sysfs "/devices/system/cpu/bus_dcvs" '
            '(u object_r sysfs_peridot_subsystem ((s0) (s0))))',
            source)
        for rail in ('ddr', 'l3', 'llcc', 'ddrqos'):
            self.assertIn(
                '(genfscon sysfs "/devices/system/cpu/bus_dcvs/'
                f'{rail}/boost_freq" '
                '(u object_r sysfs_peridot_subsystem_floor ((s0) (s0))))',
                source)
        self.assertIn(
            '(allow peridot_daemon sysfs_peridot_subsystem '
            '(file (read getattr open)))',
            source)
        self.assertIn(
            '(allow hal_power_default sysfs_peridot_subsystem_floor (file ',
            source)
        # UFS devfreq keeps peridot's controller address, not upstream's.
        for node in ('min_freq', 'max_freq'):
            self.assertIn(
                '(genfscon sysfs '
                f'"/devices/platform/soc/1d84000.ufshc/devfreq/1d84000.ufshc/{node}" ',
                source)

    def test_upstream_mediatek_residue_is_absent(self):
        sources = (
            'android/aosp/sepolicy/vendor/peridot_daemon.te',
            'android/aosp/sepolicy/vendor/genfs_contexts',
            'android/unpacked/peridot-native.cil',
        )
        for rel in sources:
            source = (ROOT / rel).read_text()
            for residue in ('dvfsrc', 'mtk_hal_power', '112b0000', 'mali'):
                self.assertNotIn(residue, source, rel)
        te = (ROOT / 'android/aosp/sepolicy/vendor/peridot_daemon.te').read_text()
        self.assertIn(
            'allow hal_power_default sysfs_peridot_subsystem_floor:file rw_file_perms;',
            te)


if __name__ == '__main__':
    unittest.main()

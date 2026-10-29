"""Security boundaries and failure evidence for native Linux artifact verification."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('linux_verification', ROOT / 'scripts/verify-linux-distribution.py')
verification = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verification)


class LinuxVerificationTests(unittest.TestCase):
    def test_process_boundary_rejects_other_executables_and_never_expands_arguments(self):
        """Shell metacharacters remain data; callers cannot select a command interpreter."""
        with patch.object(verification.subprocess, 'run') as execute:
            for command in ([], ['sh', '-c', 'touch victim'], ['/tmp/docker', 'run']):
                with self.assertRaisesRegex(ValueError, 'only docker or dpkg-deb'):
                    verification.run(command)
            execute.assert_not_called()
            execute.return_value = subprocess.CompletedProcess([], 0, 'ok', '')
            argument = '/tmp/assets; touch victim $(id)'
            self.assertEqual(verification.run(['dpkg-deb', '--raw-extract', argument, '/tmp/output']), 'ok')
            self.assertEqual(execute.call_args.args[0][2], argument)
            self.assertIs(execute.call_args.kwargs['shell'], False)

    def test_invalid_assets_publish_failure_report_before_running_tools(self):
        """Missing or corrupt artifacts cannot produce stale success or execute containers."""
        for scenario, error in [('missing', 'regular asset'), ('tampered', 'checksum')]:
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                if scenario == 'tampered':
                    archive = root / 'ts-crap-x86_64-unknown-linux-musl.tar.xz'
                    original = b'original archive bytes'
                    archive.write_bytes(original)
                    archive.with_name(archive.name + '.sha256').write_text(
                        hashlib.sha256(original).hexdigest() + '  ' + archive.name + '\n')
                    archive.write_bytes(b'tampered archive bytes')
                report = root / 'report.json'
                report.write_text('{"passed": true}')
                with patch.object(verification.subprocess, 'run') as execute:
                    with self.assertRaisesRegex(ValueError, error):
                        verification.verify(root, 'x86_64-unknown-linux-musl', '2.2.0', report)
                    execute.assert_not_called()
                result = json.loads(report.read_text())
                self.assertIs(result['passed'], False)
                self.assertIn(error, result['error'])
                self.assertEqual(result['images'], [])



if __name__ == '__main__':
    unittest.main()

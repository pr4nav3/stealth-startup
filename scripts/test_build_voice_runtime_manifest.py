import json
import importlib.util
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("build_voice_runtime_manifest.py")
SPEC = importlib.util.spec_from_file_location("build_voice_runtime_manifest", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
build_manifest = MODULE.build_manifest


class VoiceRuntimeManifestTests(unittest.TestCase):
    def test_builds_complete_manifest_and_rejects_missing_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            mac = self.write_asset(root, "macos-arm64", "supervisor-voice-worker")
            windows = self.write_asset(
                root, "windows-x64", "supervisor-voice-worker.exe"
            )

            manifest = build_manifest("runtime-1", [windows, mac])

            self.assertEqual(manifest["worker_api"], 1)
            self.assertEqual(
                [runtime["target"] for runtime in manifest["runtimes"]],
                ["macos-arm64", "windows-x64"],
            )
            with self.assertRaisesRegex(ValueError, "missing runtime targets"):
                build_manifest("runtime-1", [mac])

    def write_asset(self, root: Path, target: str, executable: str) -> Path:
        path = root / f"{target}.asset.json"
        path.write_text(
            json.dumps(
                {
                    "target": target,
                    "url": f"https://example.invalid/{target}.zip",
                    "sha256": "a" * 64,
                    "bytes": 123,
                    "executable": executable,
                }
            ),
            encoding="utf-8",
        )
        return path


if __name__ == "__main__":
    unittest.main()

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest


SCRIPTS = Path(__file__).resolve().parent
CONTEXT = (
    "ZZ_SOCKET", "ZZ_PANE", "ZZ_SESSION", "TMUX", "TMUX_PANE",
    "ZZ_TMUX_EXECUTABLE", "ZZ_APP_STARTUP_DIRECTORY", "ZZ_STARTUP_REENTRY", "ZZ_DEV_BUILD",
)
TOOL = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
root = pathlib.Path(os.environ["TEST_ROOT"])
args = sys.argv[1:]
if name == "uname":
    print(os.environ["TEST_KERNEL"])
elif name == "zig":
    print("0.16.0")
elif name == "ps":
    pass
elif name == "setsid":
    os.execv(args[1], args[1:])
elif name == "just":
    os.execv("/bin/bash", ["bash", str(root / "scripts/run.sh"), args[1]])
elif name == "cargo":
    if args[0] == "metadata":
        print(json.dumps({"target_directory": str(root / "target")}))
    else:
        (root / "build.json").write_text(json.dumps({"args": args, "env": dict(os.environ)}))
        if args[0] == "xtask":
            binaries = [root / "dist/zz-dev/zz Dev.app/Contents/MacOS/zz", root / "dist/zz-dev/zz Dev.app/Contents/MacOS/cli"]
        elif "zz_cli" in args:
            binaries = [root / "target/debug/zz_cli"]
        else:
            binaries = [root / "target/debug/zz"]
        for binary in binaries:
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.write_text("""#!/usr/bin/env python3
import json, os, pathlib, sys
pathlib.Path(os.environ["TEST_ROOT"], "launch.json").write_text(json.dumps({"args": sys.argv[1:], "env": dict(os.environ)}))
""")
            binary.chmod(0o755)
'''


class DevelopmentLaunchTests(unittest.TestCase):
    def test_dev_cli_link_preserves_an_existing_regular_file(self):
        with tempfile.TemporaryDirectory(prefix="zz dev link ") as directory:
            link = Path(directory) / ".local/bin/zz-dev"
            link.parent.mkdir(parents=True)
            link.write_text("existing command")
            result = subprocess.run(
                ["bash", str(SCRIPTS / "link-dev-cli.sh"), "/tmp/dev-executable", "/tmp/dev-gui-executable"],
                env=dict(os.environ, HOME=directory), capture_output=True, timeout=10,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(link.read_text(), "existing command")

    def launch(self, platform, watch=False):
        with tempfile.TemporaryDirectory(prefix="zz dev launch ") as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            icon = root / "assets/linux/hicolor/scalable/apps/zz-dev.svg"
            icon.parent.mkdir(parents=True)
            icon.write_text("<svg/>")
            for name in ("run.sh", "run-watch.sh", "link-dev-cli.sh"):
                shutil.copy2(SCRIPTS / name, root / "scripts" / name)
            tools = root / "tools"
            tools.mkdir()
            for name in ("cargo", "uname", "zig", "setsid", "ps", "just"):
                path = tools / name
                path.write_text(TOOL)
                path.chmod(0o755)
            environment = dict(os.environ)
            environment.update({key: "inherited-stable" for key in CONTEXT})
            environment.update(
                TEST_ROOT=str(root),
                HOME=str(root / "home"),
                XDG_DATA_HOME=str(root / "data"),
                TEST_KERNEL="Darwin" if platform == "mac" else "Linux",
                ZZ_ZIG_VERSION="0.16.0",
                ZZ_LOG_DIR="/stable/logs",
                ZZ_CARGO_FEATURES="agent-pane",
                PATH=f"{tools}:{environment['PATH']}",
            )
            script = "run-watch.sh" if watch else "run.sh"
            arguments = ["--reload"] if watch else ["--verbose"]
            subprocess.run(
                ["bash", str(root / "scripts" / script), platform, *arguments],
                env=environment, check=True, capture_output=True, timeout=10,
            )
            deadline = time.monotonic() + 5
            while not (root / "launch.json").exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            launch = json.loads((root / "launch.json").read_text())
            build = json.loads((root / "build.json").read_text())
            self.assertEqual(build["env"]["ZZ_DEV_BUILD"], "1")
            self.assertEqual(launch["args"], ["app"] if watch else ["--verbose", "app"])
            self.assertEqual(launch["env"]["ZZ_LOG_DIR"], str(root / "logs"))
            for key in CONTEXT:
                self.assertNotIn(key, launch["env"])
            self.assertEqual(build["env"]["ZZ_CARGO_FEATURES"], "agent-pane")
            self.assertTrue((root / "home/.local/bin/zz-dev").is_symlink())
            if platform == "linux":
                desktop = (root / "data/applications/zz-dev.desktop").read_text()
                self.assertIn("Icon=zz-dev", desktop)
                self.assertIn(f'Exec="{root}/target/debug/zz-dev" app', desktop)

    def test_linux_run_ignores_inherited_stable_context(self):
        self.launch("linux")

    def test_macos_run_uses_development_bundle(self):
        self.launch("mac")

    def test_linux_watch_uses_the_same_development_identity(self):
        self.launch("linux", watch=True)

    def test_macos_watch_uses_the_same_development_identity(self):
        self.launch("mac", watch=True)


if __name__ == "__main__":
    unittest.main()

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parent
TOOL = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
root = pathlib.Path(os.environ["TEST_ROOT"])
with (root / "calls.jsonl").open("a") as log:
    log.write(json.dumps({"tool": name, "args": args, "env": dict(os.environ)}) + "\n")
if name == "uname":
    print("Darwin")
elif name == "cargo" and "zz-web" in args:
    binary = root / "target/debug/zz-web"
    binary.parent.mkdir(parents=True, exist_ok=True)
    binary.write_text(pathlib.Path(sys.argv[0]).read_text())
    binary.chmod(0o755)
elif name == "rustup" and args[0] == "target":
    print("wasm32-unknown-unknown")
elif name == "wasm-bindgen" and args == ["--version"]:
    print("wasm-bindgen 0.2.128")
'''


class DevelopmentClientTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="zz dev clients ", dir="/tmp")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / "scripts").mkdir()
        for name in ("web-dev.sh", "build-web-wasm.sh"):
            shutil.copy2(SCRIPTS / name, self.root / "scripts" / name)
        (self.root / "Cargo.toml").write_text('version = "1.2.3"\n')
        for file in ("clients/web/web/index.html", "clients/web/web/main.js", "clients/web/web/style.css",
                     "assets/linux/hicolor/256x256/apps/zz.png", "assets/linux/hicolor/256x256/apps/zz-dev.png", "clients/web/assets/fonts/inter/LICENSE.txt",
                     "clients/web/assets/fonts/lilex/LICENSE.txt"):
            path = self.root / file
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture")
        tools = self.root / "tools"
        tools.mkdir()
        for name in ("uname", "cargo", "cargo-watch", "rustup", "wasm-bindgen"):
            path = tools / name
            path.write_text(TOOL.replace("#!/usr/bin/env python3", f"#!{sys.executable}", 1))
            path.chmod(0o755)
        self.environment = dict(os.environ, TEST_ROOT=str(self.root), PATH=f"{tools}:{os.environ['PATH']}",
                                ZZ_SOCKET="/stable/default.sock", ZZ_DEV_BUILD="inherited", ZZ_PANE="stable-pane",
                                XDG_RUNTIME_DIR=str(self.root / "runtime"))

    def run_script(self, script, *arguments, **environment):
        return subprocess.run(["bash", str(self.root / "scripts" / script), *arguments],
                              env=dict(self.environment, **environment), capture_output=True, text=True, timeout=10)

    def calls(self, tool):
        return [call for line in (self.root / "calls.jsonl").read_text().splitlines()
                if (call := json.loads(line))["tool"] == tool]

    def test_web_serve_discards_inherited_socket_and_forwards_explicit_argument(self):
        result = self.run_script("web-dev.sh", "--serve-only", "--socket", "/tmp/explicit.sock")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls("cargo")[-1]["env"]["ZZ_DEV_BUILD"], "1")
        launch = self.calls("zz-web")[-1]
        self.assertEqual(launch["args"], ["--socket", "/tmp/explicit.sock"])
        for key in ("ZZ_SOCKET", "ZZ_PANE", "ZZ_DEV_BUILD"):
            self.assertNotIn(key, launch["env"])

    def test_web_debug_and_release_assets_are_separate(self):
        for arguments, identity, output in (((), "1", "dist-dev"), (("--release",), "0", "dist")):
            result = self.run_script("build-web-wasm.sh", *arguments)
            self.assertEqual(result.returncode, 0, result.stderr)
            build = [call for call in self.calls("rustup") if "build" in call["args"]][-1]
            self.assertEqual(build["env"]["ZZ_DEV_BUILD"], identity)
            self.assertTrue((self.root / f"clients/web/{output}/index.html").exists())


if __name__ == "__main__":
    unittest.main()

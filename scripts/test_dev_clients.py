import json
import os
from pathlib import Path
import shutil
import socket
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
elif name == "getconf":
    print(root / "darwin")
elif name == "xcrun" and args[:3] == ["simctl", "list", "devices"]:
    print(json.dumps({"devices": {"com.apple.CoreSimulator.SimRuntime.iOS-27-0": [
        {"name": "iPhone Test", "state": "Booted", "udid": "phone"},
        {"name": "iPad Test", "state": "Booted", "udid": "pad"}]}}))
elif name == "xcodebuild":
    derived = pathlib.Path(args[args.index("-derivedDataPath") + 1])
    config = args[args.index("-configuration") + 1]
    platform = "iphonesimulator" if "Simulator" in args[args.index("-destination") + 1] else "iphoneos"
    (derived / f"Build/Products/{config}-{platform}/ZZ.app").mkdir(parents=True, exist_ok=True)
elif name == "cargo" and "zz-client-ffi" in args:
    target = args[args.index("--target") + 1]
    profile = "release" if "--release" in args else "debug"
    library = root / f"target/{target}/{profile}/libzz_client_ffi.a"
    library.parent.mkdir(parents=True, exist_ok=True)
    library.write_text(os.environ["ZZ_DEV_BUILD"])
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
        for name in ("ios-sim.sh", "ios-device.sh", "build-ios-client-core.sh", "web-dev.sh", "build-web-wasm.sh"):
            shutil.copy2(SCRIPTS / name, self.root / "scripts" / name)
        (self.root / "Cargo.toml").write_text('version = "1.2.3"\n')
        for file in ("clients/web/web/index.html", "clients/web/web/main.js", "clients/web/web/style.css",
                     "assets/linux/hicolor/256x256/apps/zz.png", "assets/linux/hicolor/256x256/apps/zz-dev.png", "clients/web/assets/fonts/inter/LICENSE.txt",
                     "clients/web/assets/fonts/lilex/LICENSE.txt"):
            path = self.root / file
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture")
        (self.root / "products").mkdir()
        tools = self.root / "tools"
        tools.mkdir()
        for name in ("uname", "getconf", "xcrun", "xcodebuild", "xcodegen", "open", "cargo", "cargo-watch", "rustup", "wasm-bindgen"):
            path = tools / name
            path.write_text(TOOL.replace("#!/usr/bin/env python3", f"#!{sys.executable}", 1))
            path.chmod(0o755)
        self.environment = dict(os.environ, TEST_ROOT=str(self.root), PATH=f"{tools}:{os.environ['PATH']}",
                                ZZ_SOCKET="/stable/default.sock", ZZ_DEV_BUILD="inherited", ZZ_PANE="stable-pane",
                                DEVELOPER_DIR=subprocess.check_output(["xcode-select", "-p"], text=True).strip() if shutil.which("xcode-select") else "/Xcode", XDG_RUNTIME_DIR=str(self.root / "runtime"), USER=f"tester-{self.root.name[-8:]}",
                                BUILT_PRODUCTS_DIR=str(self.root / "products"), ZZ_IOS_REUSE_CLIENT_CORE="0")
        self.runtime_socket = self.make_socket(self.root / "runtime/zz-dev/default.sock")

    def make_socket(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        server = socket.socket(socket.AF_UNIX)
        self.addCleanup(server.close)
        server.bind(str(path))
        return str(path)

    def run_script(self, script, *arguments, **environment):
        return subprocess.run(["bash", str(self.root / "scripts" / script), *arguments],
                              env=dict(self.environment, **environment), capture_output=True, text=True, timeout=10)

    def calls(self, tool):
        return [call for line in (self.root / "calls.jsonl").read_text().splitlines()
                if (call := json.loads(line))["tool"] == tool]

    def assert_dev_build(self, call):
        for key in ("ZZ_SOCKET", "ZZ_PANE"):
            self.assertNotIn(key, call["env"])
        for value in ("ZZ_DEV_BUILD=1", "ZZ_APP_BUNDLE_ID=dev.zz.ios.dev", "ZZ_APP_DISPLAY_NAME=zz Dev", "ZZ_APP_URL_SCHEME=zz-dev", "ZZ_APP_ICON=zz-dev"):
            self.assertIn(value, call["args"])

    def test_simulator_families_ignore_stable_socket(self):
        for family, device in (("iPhone", "phone"), ("iPad", "pad")):
            result = self.run_script("ios-sim.sh", ZZ_IOS_SIMULATOR_FAMILY=family)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assert_dev_build(self.calls("xcodebuild")[-1])
            launch = self.calls("xcrun")[-1]
            self.assertEqual(launch["args"], ["simctl", "launch", "--console-pty", device, "dev.zz.ios.dev"])
            self.assertEqual(launch["env"]["SIMCTL_CHILD_ZZ_SOCKET"], self.runtime_socket)
        terminations = [call["args"][-1] for call in self.calls("xcrun") if "terminate" in call["args"]]
        self.assertEqual(terminations, ["dev.zz.ios.dev", "dev.zz.ios.dev"])

    def test_simulator_preserves_explicit_dev_socket(self):
        explicit = self.make_socket(self.root / "explicit.sock")
        result = self.run_script("ios-sim.sh", ZZ_DEV_SOCKET=explicit)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls("xcrun")[-1]["env"]["SIMCTL_CHILD_ZZ_SOCKET"], explicit)

    def test_simulator_finds_daemon_outside_shell_temporary_directory(self):
        live = self.make_socket(self.root / f"darwin/zz-dev-{self.environment['USER']}/default.sock")
        for shell_tmp in (str(self.root / "other-temp"), ""):
            result = self.run_script("ios-sim.sh", XDG_RUNTIME_DIR="", TMPDIR=shell_tmp)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(self.calls("xcrun")[-1]["env"]["SIMCTL_CHILD_ZZ_SOCKET"], live)

    def test_simulator_finds_desktop_daemon_under_tmp(self):
        live = self.make_socket(Path("/tmp") / f"zz-dev-{self.environment['USER']}/default.sock")
        self.addCleanup(shutil.rmtree, Path(live).parent)
        result = self.run_script("ios-sim.sh", XDG_RUNTIME_DIR="", TMPDIR=str(self.root / "other-temp"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls("xcrun")[-1]["env"]["SIMCTL_CHILD_ZZ_SOCKET"], live)

    def test_simulator_rejects_missing_explicit_socket_before_build_or_launch(self):
        result = self.run_script("ios-sim.sh", ZZ_DEV_SOCKET=str(self.root / "missing.sock"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no dev daemon socket", result.stderr)
        self.assertEqual(self.calls("xcodebuild"), [])
        self.assertEqual(self.calls("xcrun"), [])

    def test_device_optimization_does_not_change_identity(self):
        for configuration in ("Debug", "Release"):
            result = self.run_script("ios-device.sh", "device-id", ZZ_IOS_CONFIGURATION=configuration)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assert_dev_build(self.calls("xcodebuild")[-1])
            self.assertEqual(self.calls("xcrun")[-1]["args"][-1], "dev.zz.ios.dev")

    def test_client_core_reuse_cannot_cross_identities(self):
        for identity in ("0", "1"):
            result = self.run_script("build-ios-client-core.sh", ZZ_DEV_BUILD=identity)
            self.assertEqual(result.returncode, 0, result.stderr)
        for identity in ("0", "1"):
            result = self.run_script("build-ios-client-core.sh", ZZ_DEV_BUILD=identity, ZZ_IOS_REUSE_CLIENT_CORE="1")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((self.root / "products/libzz_client_ffi.a").read_text(), identity)
        self.assertEqual(len(self.calls("cargo")), 2)

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

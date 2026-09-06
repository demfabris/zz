import os
import pathlib
import shutil
import subprocess
import sys
import time

sys.stdout.reconfigure(line_buffering=True)


def tmux(*args):
    result = subprocess.run(["tmux", *args], capture_output=True, timeout=120)
    if result.returncode:
        raise RuntimeError((args, result.returncode, result.stdout, result.stderr))
    return result.stdout.decode().strip()


def git(*args, cwd=None):
    result = subprocess.run(["git", *args], capture_output=True, timeout=60, cwd=cwd,
                            env=dict(os.environ, GIT_TERMINAL_PROMPT="0",
                                     GIT_CONFIG_GLOBAL="/dev/null",
                                     GIT_AUTHOR_NAME="compat",
                                     GIT_AUTHOR_EMAIL="compat@example.invalid",
                                     GIT_COMMITTER_NAME="compat",
                                     GIT_COMMITTER_EMAIL="compat@example.invalid"))
    if result.returncode:
        raise RuntimeError((args, result.returncode, result.stdout, result.stderr))
    return result.stdout.decode().strip()


def settle(probe, want, seconds=30):
    deadline = time.monotonic() + seconds
    seen = None
    while time.monotonic() < deadline:
        seen = probe()
        if seen == want:
            return seen
        time.sleep(0.05)
    raise AssertionError((want, seen))


side = "zz" if os.environ.get("ZZ_SMOKE_ZZ_BIN") else "tmux"
home = pathlib.Path(os.environ["HOME"])
root = home / ("tpm-install-" + side)
shutil.rmtree(root, ignore_errors=True)
root.mkdir(parents=True)
plugins = home / ".tmux/plugins"
tpm = plugins / "tpm"
installed = plugins / "zz-compat-plugin"
shutil.rmtree(installed, ignore_errors=True)
result = "failed:" + os.environ.get("ZZ_SMOKE_CANARY", "unknown")

source = root / "zz-compat-plugin"
source.mkdir()
(source / "zz-compat-plugin.tmux").write_text(
    "#!/usr/bin/env bash\ntmux set-option -g @zz-compat-plugin-loaded yes\n")
(source / "zz-compat-plugin.tmux").chmod(0o755)
git("init", "-q", "-b", "main", str(source))
git("add", "-A", cwd=source)
git("commit", "-qm", "compat plugin", cwd=source)

config = home / ".tmux.conf"
config.write_text(
    "set -g @plugin '%s'\nrun-shell '%s'\n" % (source, tpm / "tpm"))

try:
    tmux("set-option", "-g", "status", "off")
    tmux("source-file", str(config))
    print("TPM_MANAGER_PATH=" + tmux("show-environment", "-g",
                                     "TMUX_PLUGIN_MANAGER_PATH")
          .replace(str(home), "<HOME>"))

    rows = tmux("list-keys", "-T", "prefix", "-F", "#{key_string}\t#{key_command}")
    binding = [row.split("\t", 1)[1] for row in rows.splitlines()
               if row.split("\t", 1)[0] == "I"]
    if len(binding) != 1:
        raise AssertionError(("one prefix I binding", rows))
    binding = binding[0]
    print("TPM_INSTALL_BINDING=" + binding.replace(str(home), "<HOME>"))
    print("TPM_INSTALLED_BEFORE=%s" % installed.exists())

    tmux("run-shell", "-C", binding)
    settle(lambda: (installed / "zz-compat-plugin.tmux").exists(), True)
    print("TPM_INSTALLED_AFTER=%s" % installed.is_dir())
    print("TPM_INSTALLED_FILES=" + ",".join(
        sorted(entry.name for entry in installed.iterdir() if entry.name != ".git")))
    print("TPM_CLONE_HEAD_SUBJECT=" + git("log", "-1", "--format=%s", cwd=installed))

    tmux("set-option", "-gu", "@zz-compat-plugin-loaded")
    tmux("source-file", str(config))
    print("TPM_SOURCED_OPTION=" + settle(
        lambda: tmux("show-options", "-gqv", "@zz-compat-plugin-loaded"), "yes"))
    result = "clean:prefix-I-cloned-and-sourced"
except Exception as error:
    print(repr(error), flush=True)
finally:
    tmux("set-environment", "-g", "ZZ_TPM_INSTALL", result)

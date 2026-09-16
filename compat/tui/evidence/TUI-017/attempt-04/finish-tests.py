from pathlib import Path
import subprocess

p = Path.cwd() / 'compat/tui/evidence/TUI-017/attempt-04'
base = ['env', 'CARGO_HOME=/home/demfabris/.cargo', 'RUSTUP_HOME=/home/demfabris/.rustup', 'HOME=/tmp/zz-emptyhome', 'XDG_CONFIG_HOME=/tmp/zz-emptyhome/config', '/tmp/zz-cargo.sh']
commands = [
    ('07-daemon-callback-solo', [*base, 'test', '-p', 'zz-daemon', '--lib', 'control_background_callbacks_cancel_after_disconnect']),
    ('07-daemon-shell-solo', [*base, 'test', '-p', 'zz-daemon', '--lib', 'default_shell_rejects_invalid_values_and_unset_controls_the_child_shell']),
    ('07-daemon-socks-solo', [*base, 'test', '-p', 'zz-daemon', '--lib', 'loopback_forwards_http_and_tcp_in_both_families_with_original_port']),
    ('08-packages-remaining', [*base, 'test', '-p', 'zz-mux', '-p', 'zz-terminal', '-p', 'zz-protocol']),
    ('08-daemon-integrations', [*base, 'test', '-p', 'zz-daemon', '--test', '*'])
]
for label, command in commands:
    subprocess.run(['python3', str(p / 'run.py'), label, *command])

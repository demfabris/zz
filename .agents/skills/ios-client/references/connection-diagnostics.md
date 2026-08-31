# Physical-device connection diagnostics

Use this reference when an iPhone or iPad says unreachable, keeps reconnecting, fails after SSH
authentication, or connects to the wrong daemon.

Start from the exact device, saved endpoint, visible error, time window, and host. Keep the first pass
read-only. Do not change Remote Login, firewall rules, authorized keys, saved host trust, or daemon
state until the evidence identifies that layer.

If the user gives no device, use the sole connected physical device when only one exists. Ask which
device to inspect when several are connected. Prefer its CoreDevice identifier in commands because
the current `just ios-device` recipe does not preserve whitespace in names.

## Transport model

There is no zz TCP listener to expose on the LAN.

```text
iPhone or iPad
  -> LAN TCP 22
  -> in-process russh
  -> remote shell probe
  -> optional zz daemon start
  -> zz proxy over SSH channel stdio
  -> host Unix socket
  -> zz daemon
```

The source path is:

- `InteractiveClient::connect_endpoint_with_prompts_and_terminal` in
  `crates/zz-daemon/src/client.rs`
- `RusshForward::start` and `establish` in `crates/zz-daemon/src/russh_client.rs`
- `REMOTE_SOCKET_PROBE`, `remote_daemon_start_script`, `remote_proxy_script`, and
  `remote_path_fallback!` in `crates/zz-daemon/src/endpoint.rs`

Physical-device endpoints require an explicit user, such as `ssh://user@host`. The app stores the
normalized endpoint under `zz.saved-host`.

The app keeps its Ed25519 private key in Keychain and exposes the public half in host setup.
Authentication tries that identity, keyboard-interactive prompts, and password when the server
offers them. A setup password stays in memory for that attempt. Host-key prompts support reject,
trust once, and trust/save.

Remote scripts append these install locations before they look up `zz`:

```text
$HOME/.local/bin
/opt/homebrew/bin
/usr/local/bin
```

The probe compares protocol versions before it opens `zz proxy`. Without an explicit socket in the
endpoint, it starts the remote daemon when needed. An explicit socket disables auto-start and must
already be usable.

## Evidence ladder

### 1. Device state

Confirm the device is paired, available through CoreDevice, unlocked, and running with Developer
Mode enabled:

```sh
xcrun devicectl list devices
xcrun devicectl device info details --device <device-id>
xcrun devicectl device info lockState --device <device-id>
```

Wireless Xcode pairing and app traffic are different paths. A paired device still needs LAN access
to the host's SSH service. Keep `NSLocalNetworkUsageDescription` in the generated app metadata.

### 2. Installed app, process, and saved endpoint

```sh
xcrun devicectl device info apps --device <device-id> --bundle-id dev.zz.ios
xcrun devicectl device info processes --device <device-id> --search ZZ
```

Copy the app preferences without changing them. Use an unused destination path:

```sh
xcrun devicectl device copy from \
  --device <device-id> \
  --domain-type appDataContainer \
  --domain-identifier dev.zz.ios \
  --source Library/Preferences/dev.zz.ios.plist \
  --destination /tmp/dev.zz.ios.plist

plutil -p /tmp/dev.zz.ios.plist
```

This copies data into a local `/tmp` artifact but does not change the device container. Skip it under
a no-write request unless the user approves that local artifact.

Verify `zz.saved-host` contains the intended `ssh://user@host`. Resolve that exact host from the Mac
and compare it with the Mac's current LAN address.

### 3. SSH reachability and authentication

Confirm port 22 on the exact target:

```sh
nc -G 2 -zv <host-or-lan-ip> 22
```

Inspect a bounded macOS SSH log window after reproducing the failure:

```sh
log show --last 10m --style compact \
  --predicate '(process == "sshd") OR (process == "sshd-session")'
```

Use a live stream only while coordinating one attempt, then stop it:

```sh
log stream --style compact \
  --predicate '(process == "sshd") OR (process == "sshd-session")'
```

An `Accepted publickey` line for the app's key proves the device reached the right host, DNS or the
literal address worked, LAN routing and port 22 worked, host trust completed, and authentication
succeeded. Repeated accepted keys at the reconnect interval move the investigation after
authentication. Do not keep changing firewall, Local Network permission, or authorized keys.

No accepted connection keeps DNS, host address, Local Network permission, port 22, host trust, and
authentication in scope. Use the app's displayed transport error to narrow them.

### 4. Remote command path, protocol, and socket

Run the same path policy on the host:

```sh
sh -lc 'PATH="$PATH:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin"; export PATH; command -v zz; zz protocol-version'
```

Confirm the located binary matches the intended installation and the protocol matches the device
core. Check the probed daemon socket and the currently running daemon. The daemon survives desktop
app replacement and can keep serving older code.

Do not run an unbounded manual `zz proxy` as a casual probe; it owns a live stdio session. If proxy
inspection is required, capture its process and SSH parent, bound the observation, and terminate only
the diagnostic process you started.

Correlate an existing proxy with its SSH parent and socket without changing it:

```sh
ps -axo pid,ppid,lstart,command | rg '[z]z proxy|sshd-session'
lsof -nP -p <proxy-pid>
```

### 5. Installed Rust core

Simulator and physical archives are separate. A successful simulator build says nothing about the
installed `aarch64-apple-ios` core. A diagnose-only request needs explicit approval before this
build, install, and launch. Rebuild without inherited reuse:

```sh
env -u ZZ_IOS_REUSE_CLIENT_CORE just ios-device <device-id>
```

If a post-authentication retry loop disappears after this build, the installed static core was
stale. This exact failure occurred when the device archive predated the remote PATH fallback: SSH
accepted the iPad key every retry, while the old core failed before a persistent proxy connection.

### 6. Prove application state

Capture a screenshot after launch. The command writes a local `/tmp` file; skip it under a no-write
request unless the user approves that artifact:

```sh
xcrun devicectl device capture screenshot \
  --device <device-id> \
  --destination /tmp/zz-device.png
```

Connected proof includes a real session, window, pane, and live terminal or Agent state. Build,
installation, process launch, and an SSH acceptance line are intermediate facts.

## Failure classification

The FFI's `classify_connect_error` controls retry behavior.

| Failure | App behavior |
| --- | --- |
| Authentication, rejected host key, invalid endpoint or socket, missing remote `zz`, protocol mismatch | Stop automatic retry and return to setup or failure UI |
| DNS, TCP or SSH transport, probe, daemon startup timeout, proxy startup or channel failure | Retry with retained presentation |

The mobile backoff is 1, 2, 4, 8, then 16 seconds and remains capped at 16. Network restoration starts
the next attempt without waiting. The reconnect page and retained-workspace banner preserve the last
error while the next attempt runs.

## Symptom map

| Symptom | Highest-value next check |
| --- | --- |
| App absent | Signing, install output, bundle ID |
| Build and install succeed, launch error 7 | Unlock the device, then relaunch |
| No SSH log entry | Saved host, DNS or IP, LAN permission, port 22 |
| SSH rejects the key | Copied app public key and host `authorized_keys` |
| SSH accepts once, app reports incompatible | Device core and daemon protocol versions |
| SSH accepts on every backoff, app keeps reconnecting | Remote probe, `zz` path, daemon socket, proxy, stale physical archive |
| App connects but shows no panes | Attachment event, selected session, authoritative snapshot, visible-pane scope |
| New code appears absent after install | Reused device archive or long-lived old daemon |

End diagnosis with one plain verdict: which layers are proven healthy, which exact layer failed, and
what evidence proves the connected state after the fix.

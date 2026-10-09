---
type: Rust Crate
title: zz-daemon-client crate
description: The client half of the zz daemon. Local and ssh endpoints, ssh askpass, the command and interactive clients, and the transport, config paths, and process facts the server shares.
resource: crates/zz-daemon-client/src/lib.rs
tags: [crate, client, ipc, transport, ssh, endpoint]
timestamp: 2026-10-09T00:00:00-03:00
---

# Overview

`zz-daemon-client` is everything a process needs to talk to a [zz daemon](/crates/zz-daemon.md)
without hosting one: resolve an endpoint, reach it over the local socket or an ssh tunnel, answer
ssh's prompts, run commands with `CommandClient`, and attach with `InteractiveClient`. It was split
out of `zz-daemon` on 2026-10-09. Before that, client-only crates depended on `zz-daemon` with
`default-features = false`, so every edit to the server recompiled them, and in a whole-workspace
build feature unification pulled the full server into their graph anyway.

Who depends on what:

| Crate | Depends on |
|-------|-----------|
| `zz-tui`, `zz-config`, `zz-app` (iOS), `zz-ios`, `zz-web` | `zz-daemon-client` only |
| `zz`, `zz-cli` | both: they run `Daemon` and they are clients |
| `zz-daemon` | `zz-daemon-client`, for what both halves share |
| `zz-client`, `zz-web` tests | `zz-daemon` as a dev-dependency, to spawn a real daemon |

The crate has no cargo features. It carries the iOS in-process ssh client (`russh`), so
`IPHONEOS_DEPLOYMENT_TARGET=26.0 cargo clippy -p zz-ios -p zz-app --target aarch64-apple-ios`
is the check that covers those modules.

# What the server takes from it

The server uses these and nothing else from the client half:

| Item | Server use |
|------|------------|
| `DaemonError`, `strerror_text` | every failure either side can see, and tmux-style IO error text |
| `transport::{LocalTransport, LocalListener, LocalStream, Transport, TransportListener, TransportStream}` | binding and accepting the owner-only socket |
| `DaemonIdentityGuard` | writing the identity file next to the socket; its reader, `terminate_incompatible_daemon`, lives in the same module so the format stays in one place |
| `process_info`, `unmasked::SpawnUnmasked`, `user_data` | pane process facts, unmasked spawns, the agent journal directory |
| `default_mux_config`, `discover_tmux_config`, `mux_config_write_path`, `home_directory` | config discovery, `~` expansion, and the home-directory fallback |
| `TERMINAL_FEATURES`, `terminal_features_list`, `terminal_feature_mask`, `terminal_colour_count` | `client_termfeatures` and `client_colours` |
| `STARTUP_REENTRY_*`, `CLIENT_EXITS_ON_DETACH_CAPABILITY`, `COLD_START_PREPARE_ABORT_COMMAND` | hello capabilities and startup handoff both ends must spell the same way |
| `diagnostic_timer`, `diagnostic_elapsed_us`, `TracedMessage`, `atomic_write`, `shell_quote` | diagnostics timing, message tracing that redacts question answers, `import-tmux-config` rewriting `zz/mux.conf`, test fixtures |

The `mio` waker plumbing (`AcceptWake`, `wake_loop`, `LoopThread`) stays in `zz-daemon`, since
only the event loop uses it.

# Module map

| Module (`crates/zz-daemon-client/src/`) | Public surface | Role |
|-----------------------------------------|----------------|------|
| `lib.rs` | `DaemonError`, `classify_local_connect_error`, the shared constants, and re-exports of the modules below | Crate root |
| `client.rs` | `CommandClient`, `InteractiveClient`, `ExecChain`, `short_device_name`, the client terminal-feature state | Client halves of the protocol: connect + handshake (`connect_endpoint` for an `ssh://` endpoint), endpoint-scoped cwd/tty/size/nested/environment hello facts, framed `ProtocolSender`/`ProtocolReceiver`, request/response and attach/detach/input helpers |
| `transport.rs` | `default_socket_path`, the `transport` module | Platform IPC: wraps `interprocess` into `LocalListener`/`LocalStream`, per-platform endpoint paths, peer-credential capture |
| `endpoint.rs` | `Endpoint`, `SshEndpoint`, `EndpointError`, `run_socket_proxy` | `unix://`/bare-path/`ssh://` parsing and the probe, auto-start, forward ssh sequence described in [zz-daemon](/crates/zz-daemon.md#reaching-a-remote-daemon-over-ssh) |
| `askpass.rs` | `SshPrompts`, `AskpassPrompt`, `AskpassPromptKind`, `AskpassReply`, `ASKPASS_SOCKET_ENV`, `run_helper` | ssh's password and host-key prompts: the per-connect socket the GUI answers on, the prompt classifier, and the helper mode `zz` re-enters when ssh runs it as `SSH_ASKPASS` |
| `lifecycle.rs` | `DaemonIdentityGuard`, `RecoveredDaemon`, `DaemonRecoveryError`, `terminate_incompatible_daemon`, `daemon_identity_protocol_version` | Single-instance identity file and guarded termination of an incompatible-protocol daemon |
| `paths.rs` | `default_mux_config`, `discover_tmux_config`, `mux_config_candidates`, `mux_config_write_path`, `home_directory` | zz-owned config paths and donor discovery for explicit tmux imports |
| `fleet_hosts.rs` | `HostEntry`, `RejectedHost`, `configured_fleet_hosts`, `validate_fleet_host`, `write_fleet_host`, `apply_fleet_host_entry` | `host-<name>` entries in `zz/config` |
| `terminal_features.rs` | `TERMINAL_FEATURES`, `terminal_feature_mask`, `terminal_features_list`, `terminal_colour_count`, `terminal_default_features` | The `tty-features.c` table and the colours it decides |
| `process_info.rs` | the `process_info` module | Per-OS process facts: start time, command name, cwd, parent, process group, samples, host name |
| `unmasked.rs` | `unmasked::SpawnUnmasked` | The only way zz starts a `std::process::Command`: `spawn_unmasked`, `output_unmasked` and `status_unmasked` clear the calling thread's signal mask for the spawn and put it back after. Since Rust 1.97 a child inherits the mask of the thread that spawned it, and GPUI's background threads block nearly every signal, so a plain spawn from the desktop app starts ssh, shells and helpers with SIGINT, SIGTERM, SIGCHLD and SIGWINCH blocked. `clippy.toml` disallows the plain `spawn`, `output` and `status`; tests and `zz-xtask` are exempt. Children the daemon's job registry owns start through `daemon::jobs::spawn` in `zz-daemon` instead |
| `user_data.rs` | `platform_data_dir`, `restrict_to_current_user`, `restrict_directory_to_current_user` | Where user-owned application data lives and how it is permission-hardened; `crates/zz/src/user_data.rs` re-exports it and the daemon's agent journal answers to it too |
| `russh_client.rs`, `russh_prompt.rs`, `russh_socks.rs`, `ios_keychain.rs` (iOS) | `ios_ssh_public_key` | iOS cannot spawn ssh, so it tunnels in process with `russh`, keeps its identity in the Keychain, and runs browser egress through an in-process SOCKS forward |

# Logging

Records that name a target keep their old spelling (`zz_daemon::russh`, `zz_daemon::askpass`,
`zz_daemon::diagnostics::*`). Records without one now carry `zz_daemon_client::<module>`. The
`zz_daemon=` directives in `zz-cli`'s log filters still match both, because `env_logger` matches a
directive as a target prefix.

# Key files

| File | Role |
|------|------|
| `crates/zz-daemon-client/src/lib.rs` | Crate root, `DaemonError`, connect-error classification, public re-exports |
| `crates/zz-daemon-client/src/client.rs` | `CommandClient`, `InteractiveClient`, framed sender and receiver, handshake, post-spawn Control startup ownership |
| `crates/zz-daemon-client/src/transport.rs` | `LocalTransport`, `LocalListener`/`LocalStream` over `interprocess`, `default_socket_path`, `PeerCredentials` |
| `crates/zz-daemon-client/src/endpoint.rs` | Endpoint parsing, the ssh probe/auto-start/forward commands and their shell quoting, `SshForward`'s RAII child, `EndpointError` advice |
| `crates/zz-daemon-client/src/lifecycle.rs` | Identity file writer and reader, guarded shutdown of an incompatible daemon |
| `crates/zz-daemon-client/Cargo.toml` | No features; `russh`, `tokio`, and the Keychain bindings only on iOS |

# Related

- The server: [zz-daemon](/crates/zz-daemon.md).
- The wire vocabulary both halves speak: [zz-protocol](/crates/zz-protocol.md),
  [wire protocol](/protocol/wire-protocol.md).
- The renderer-free client state machine that sits on top of these connections:
  [zz-client](/crates/zz-client.md).
- [Process model](/architecture/process-model.md) for which processes run which half.

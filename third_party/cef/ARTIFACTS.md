# CEF artifact lock

zz resolves `cef` and `cef-dll-sys` to `152.2.0+152.0.6` in `Cargo.lock`.
That release maps to CEF `152.0.6+g708dc14+chromium-152.0.7977.83`.
`download-cef` verifies the selected minimal distribution against the SHA-1
published in CEF's official `index.json` before extracting it.

| Rust target | Minimal distribution | SHA-1 |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `cef_binary_152.0.6+g708dc14+chromium-152.0.7977.83_linux64_minimal.tar.bz2` | `9711b86c105fb590da576fe5a829802f1a79d520` |
| `aarch64-unknown-linux-gnu` | `cef_binary_152.0.6+g708dc14+chromium-152.0.7977.83_linuxarm64_minimal.tar.bz2` | `d05e22542515b1022820651c75ba0c91fb9d8ad2` |
| `arm-unknown-linux-gnueabi` | `cef_binary_152.0.6+g708dc14+chromium-152.0.7977.83_linuxarm_minimal.tar.bz2` | `55ce872fd4fe9a06f83644151e6587ca31493a8d` |
| `x86_64-apple-darwin` | `cef_binary_152.0.6+g708dc14+chromium-152.0.7977.83_macosx64_minimal.tar.bz2` | `fed74cac2af95dec716000e9a35ebed8eb4f9a25` |
| `aarch64-apple-darwin` | `cef_binary_152.0.6+g708dc14+chromium-152.0.7977.83_macosarm64_minimal.tar.bz2` | `426836139b0ea7b7278aa0915cfae90eb460551f` |
| `x86_64-pc-windows-msvc` | `cef_binary_152.0.6+g708dc14+chromium-152.0.7977.83_windows64_minimal.tar.bz2` | `e5e3020627f4528bd43e22f4c4970000b0458e99` |
| `aarch64-pc-windows-msvc` | `cef_binary_152.0.6+g708dc14+chromium-152.0.7977.83_windowsarm64_minimal.tar.bz2` | `7bf72d53750608d7f733f7488dbb80813bf043fb` |
| `i686-pc-windows-msvc` | `cef_binary_152.0.6+g708dc14+chromium-152.0.7977.83_windows32_minimal.tar.bz2` | `3e7d117c24da4847fefacb6bdf694de29a595d43` |

The values above were read from `https://cef-builds.spotifycdn.com/index.json`
on 2026-09-12. This table is a reviewable mirror; `download-cef` fetches and
verifies against the official index directly. When updating CEF, refresh the
Cargo lock, this table, the CI cache key, and all three platform bundle
checks together.

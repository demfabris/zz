# CEF artifact lock

zz resolves `cef` and `cef-dll-sys` to `154.0.0+154.0.23` in `Cargo.lock`.
That release maps to CEF `154.0.23+g062ebe4+chromium-154.0.8037.17`.
`download-cef` verifies the selected minimal distribution against the SHA-1
published in CEF's official `index.json` before extracting it.

| Rust target | Minimal distribution | SHA-1 |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `cef_binary_154.0.23+g062ebe4+chromium-154.0.8037.17_linux64_minimal.tar.bz2` | `01c4df39ce79c441950cd8bf2f385c0b886bd5d3` |
| `aarch64-unknown-linux-gnu` | `cef_binary_154.0.23+g062ebe4+chromium-154.0.8037.17_linuxarm64_minimal.tar.bz2` | `08c08229946992685b90b50ee08e7a59e192b601` |
| `arm-unknown-linux-gnueabi` | `cef_binary_154.0.23+g062ebe4+chromium-154.0.8037.17_linuxarm_minimal.tar.bz2` | `736a5b863dbbab85c72c60620473ac48fe30fec9` |
| `x86_64-apple-darwin` | `cef_binary_154.0.23+g062ebe4+chromium-154.0.8037.17_macosx64_minimal.tar.bz2` | `ae3b583f32a980f373d32e82e120eac45848d1fb` |
| `aarch64-apple-darwin` | `cef_binary_154.0.23+g062ebe4+chromium-154.0.8037.17_macosarm64_minimal.tar.bz2` | `9fdef241a8c682d98743c9fc6b27f5e54045551e` |
| `x86_64-pc-windows-msvc` | `cef_binary_154.0.23+g062ebe4+chromium-154.0.8037.17_windows64_minimal.tar.bz2` | `d9f11d9175a5b5daffc2fbe82e695c1ab05aff22` |
| `aarch64-pc-windows-msvc` | `cef_binary_154.0.23+g062ebe4+chromium-154.0.8037.17_windowsarm64_minimal.tar.bz2` | `4a5bebb6325722163aadb92d453e6a67987a929c` |
| `i686-pc-windows-msvc` | `cef_binary_154.0.23+g062ebe4+chromium-154.0.8037.17_windows32_minimal.tar.bz2` | `3b8c1169ff9193dc624872914a9627d0e6890df2` |

The values above were read from `https://cef-builds.spotifycdn.com/index.json`
on 2026-09-25. The index lists 154.0.23 twice, as a beta build and a stable
rebuild with different archives; `download-cef` takes the first match, which is
the stable build above. This table is a reviewable mirror; `download-cef` fetches and
verifies against the official index directly. When updating CEF, refresh the
Cargo lock, this table, the CI cache key, and all three platform bundle
checks together.

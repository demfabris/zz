# CEF artifact lock

zz resolves `cef` and `cef-dll-sys` to `151.2.0+151.3.14` in `Cargo.lock`.
That release maps to CEF `151.3.14+g5d67476+chromium-151.0.7922.72`.
`download-cef` verifies the selected minimal distribution against the SHA-1
published in CEF's official `index.json` before extracting it.

| Rust target | Minimal distribution | SHA-1 |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `cef_binary_151.3.14+g5d67476+chromium-151.0.7922.72_linux64_minimal.tar.bz2` | `e60272fc43cdb3e0dedae768df8df2fcfac5624d` |
| `aarch64-unknown-linux-gnu` | `cef_binary_151.3.14+g5d67476+chromium-151.0.7922.72_linuxarm64_minimal.tar.bz2` | `abf7821852d8bc304ece5ff859005e577292b0dc` |
| `arm-unknown-linux-gnueabi` | `cef_binary_151.3.14+g5d67476+chromium-151.0.7922.72_linuxarm_minimal.tar.bz2` | `e6bab47596b69c367ceb56e52784afbae9f6c11b` |
| `x86_64-apple-darwin` | `cef_binary_151.3.14+g5d67476+chromium-151.0.7922.72_macosx64_minimal.tar.bz2` | `d933f22df54d9dcffa2ecf8f4a412551c86de3fe` |
| `aarch64-apple-darwin` | `cef_binary_151.3.14+g5d67476+chromium-151.0.7922.72_macosarm64_minimal.tar.bz2` | `41c8a20b68d36b795d16287d9f75ca8ff9dc1363` |
| `x86_64-pc-windows-msvc` | `cef_binary_151.3.14+g5d67476+chromium-151.0.7922.72_windows64_minimal.tar.bz2` | `96abc7e46d7dfe31756be682e1c0d423807b498e` |
| `aarch64-pc-windows-msvc` | `cef_binary_151.3.14+g5d67476+chromium-151.0.7922.72_windowsarm64_minimal.tar.bz2` | `d2ce65af275f76f05d84f874a7cb8ff06e351038` |
| `i686-pc-windows-msvc` | `cef_binary_151.3.14+g5d67476+chromium-151.0.7922.72_windows32_minimal.tar.bz2` | `ac724b293ea94124dbdd802da3300d83afd7f593` |

The values above were read from `https://cef-builds.spotifycdn.com/index.json`
on 2026-08-02. This table is a reviewable mirror; `download-cef` fetches and
verifies against the official index directly. When updating CEF, refresh the
Cargo lock, this table, the CI cache key, and all three platform bundle smoke
tests together.

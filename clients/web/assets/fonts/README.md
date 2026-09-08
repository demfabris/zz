# Browser fallback fonts

The browser cannot access installed fonts. Register these files with GPUI's text
system before opening the window so terminal cells and shared widgets can render
Chinese, Japanese, Korean, and emoji alongside Inter and Lilex.

| File | Upstream source | License |
| --- | --- | --- |
| `NotoSansCJKjp-Regular.otf` | [Noto Sans CJK, revision f8d1575](https://github.com/notofonts/noto-cjk/blob/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans/OTF/Japanese/NotoSansCJKjp-Regular.otf) | [SIL OFL 1.1](../../web/licenses/noto-sans-cjk.txt) |
| `NotoColorEmoji.ttf` | [Noto Emoji, revision 8998f5d](https://github.com/googlefonts/noto-emoji/blob/8998f5dd683424a73e2314a8c1f1e359c19e8742/fonts/NotoColorEmoji.ttf) | [SIL OFL 1.1](../../web/licenses/noto-color-emoji.txt) |

The single regular CJK face includes Chinese, Japanese, and Korean glyphs with
Japanese regional forms. Noto Color Emoji uses the bitmap format supported by
GPUI's Swash renderer. Neither file has been modified.

SHA-256 checksums:

```text
68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5  NotoSansCJKjp-Regular.otf
72a635cb3d2f3524c51620cdde406b217204e8a6a06c6a096ff8ed4b5fd6e27b  NotoColorEmoji.ttf
```

# Browser client fonts

The browser has no access to host fonts, so the client bundles every face it renders.

| File | Face | License |
| --- | --- | --- |
| `inter/InterVariable.ttf` | Inter Variable upright | [`inter/LICENSE.txt`](inter/LICENSE.txt) |
| `inter/InterVariable-Italic.ttf` | Inter Variable italic | [`inter/LICENSE.txt`](inter/LICENSE.txt) |
| `lilex/Lilex-Regular.ttf` | Lilex regular | [`lilex/LICENSE.txt`](lilex/LICENSE.txt) |
| `lilex/Lilex-Bold.ttf` | Lilex bold | [`lilex/LICENSE.txt`](lilex/LICENSE.txt) |
| `lilex/Lilex-Italic.ttf` | Lilex italic | [`lilex/LICENSE.txt`](lilex/LICENSE.txt) |
| `lilex/Lilex-BoldItalic.ttf` | Lilex bold italic | [`lilex/LICENSE.txt`](lilex/LICENSE.txt) |

Inter comes from [rsms/inter](https://github.com/rsms/inter) and Lilex from
[mishamyrt/Lilex](https://github.com/mishamyrt/Lilex), both under the SIL Open Font
License 1.1. Chrome text uses Inter; terminals, code, and command output use Lilex
regardless of the font families the daemon's terminal appearance names, because those
are host fonts. The client ships no CJK or emoji fallback; unsupported glyphs render as
missing glyphs.

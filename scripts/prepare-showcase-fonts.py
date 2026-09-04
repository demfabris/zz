import hashlib
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CACHE = ROOT / "target" / "ui-showcase-fonts"
SOURCES = [Path("/System/Library/Fonts/SFNS.ttf"), Path("/System/Library/Fonts/SFNSItalic.ttf")]
VERSION = "4.60.2"
FAMILY = "zz Preview System"
WEIGHTS = [("regular", 400, 400), ("medium", 500, 510), ("semibold", 600, 590), ("bold", 700, 700)]
ITALIC_AXES = {400: (400, 400), 500: (508, 436), 600: (590.8, 419.1999), 700: (700, 430)}


def main():
    if sys.platform != "darwin" or not all(path.is_file() for path in SOURCES):
        return
    fingerprint = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    stamp = {"script": fingerprint, "sources": [str(path.stat().st_mtime_ns) for path in SOURCES]}
    manifest = CACHE / "manifest.json"
    outputs = [CACHE / f"{name}{suffix}.ttf" for name, _, _ in WEIGHTS for suffix in ("", "-italic")]
    if manifest.is_file() and json.loads(manifest.read_text()) == stamp and all(path.is_file() for path in outputs):
        return
    python = CACHE / "venv" / "bin" / "python"
    if Path(sys.executable) != python:
        CACHE.mkdir(parents=True, exist_ok=True)
        if not python.is_file():
            subprocess.run([sys.executable, "-m", "venv", str(CACHE / "venv")], check=True)
        subprocess.run([str(python), "-m", "pip", "install", "--disable-pip-version-check", f"fonttools=={VERSION}"], check=True)
        subprocess.run([str(python), str(Path(__file__).resolve())], check=True)
        return

    from fontTools.ttLib import TTFont
    from fontTools.varLib.instancer import instantiateVariableFont

    for italic, source in enumerate(SOURCES):
        for name, weight, outline_weight in WEIGHTS:
            font = TTFont(source)
            axes = {axis.axisTag: axis.defaultValue for axis in font["fvar"].axes}
            axes.update(opsz=17, wght=outline_weight)
            if italic:
                axes["wght"], axes["YAXS"] = ITALIC_AXES[weight]
            font = instantiateVariableFont(font, axes, inplace=True)
            style = name.title() + (" Italic" if italic else "")
            postscript = "zzPreviewSystem-" + style.replace(" ", "")
            names = {1: FAMILY, 2: style, 3: postscript, 4: f"{FAMILY} {style}", 6: postscript, 16: FAMILY, 17: style}
            table = font["name"]
            table.names = [record for record in table.names if record.nameID not in names]
            for name_id, value in names.items():
                table.setName(value, name_id, 3, 1, 0x409)
                table.setName(value, name_id, 1, 0, 0)
            font["OS/2"].usWeightClass = weight
            font["OS/2"].fsSelection &= ~0x61
            font["OS/2"].fsSelection |= (1 if italic else 0) | (0x20 if weight == 700 else 0)
            if not italic and weight != 700:
                font["OS/2"].fsSelection |= 0x40
            font["head"].macStyle = (1 if weight == 700 else 0) | (2 if italic else 0)
            output = CACHE / f"{name}{'-italic' if italic else ''}.ttf"
            pending = output.with_suffix(".tmp")
            font.save(pending)
            pending.replace(output)
            print(f"Prepared {output.name}", flush=True)
    manifest.write_text(json.dumps(stamp))


if __name__ == "__main__":
    main()

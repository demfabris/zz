import argparse
import hashlib
from pathlib import Path
import re
import subprocess


parser = argparse.ArgumentParser()
parser.add_argument("source", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
source = args.source.read_bytes()
expected_hash = "f728edbd534f5b0dceb304c4d5da72d087ffab40cb522bd3f987210694c1cf85"
if hashlib.sha256(source).hexdigest() != expected_hash:
    raise SystemExit("bindings differ from the published cef-dll-sys 152.2.0+152.0.6 source")

functions = []
for block in re.findall(r'unsafe extern "C" \{\n(.*?)\n\}', source.decode(), re.DOTALL):
    declarations = re.findall(r'^    pub fn (.*?);', block, re.MULTILINE | re.DOTALL)
    if len(declarations) != 1:
        raise SystemExit("unexpected foreign declaration layout")
    declaration = " ".join(declarations[0].split())
    if "..." in declaration:
        raise SystemExit("variadic CEF API requires an explicit loader design")
    functions.append(declaration)
if len(functions) != 194:
    raise SystemExit(f"expected 194 CEF functions, found {len(functions)}")

outputs = {
    "functions.rs": "cef_functions! {\n" + "\n".join(f"    fn {function};" for function in functions) + "\n}\n",
    "exports.rs": "pub use dynamic::{\n    is_library_loaded, load_library,\n" + "\n".join(f"    {function.split('(')[0]}," for function in functions) + "\n};\n",
}
for name, generated in outputs.items():
    path = Path(__file__).parent / "src" / name
    formatted = subprocess.run(
        ["rustfmt", "--edition", "2021", "--emit", "stdout"],
        input=generated,
        text=True,
        check=True,
        capture_output=True,
    ).stdout
    if args.check:
        if path.read_text() != formatted:
            raise SystemExit(f"{path} needs regeneration")
    else:
        path.write_text(formatted)
print(f"Verified {len(functions)} pinned CEF signatures")

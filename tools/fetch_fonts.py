"""Download the preview fonts listed in examples/fonts/manifest.json and verify their SHA-256."""
import hashlib
import json
import pathlib
import urllib.request

DIR = pathlib.Path(__file__).resolve().parent.parent / "examples" / "fonts"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None


for font in json.loads((DIR / "manifest.json").read_text())["fonts"]:
    path = DIR / font["file"]
    if digest(path) == font["sha256"]:
        continue
    tmp = path.with_suffix(".part")
    urllib.request.urlretrieve(font["url"], tmp)
    if digest(tmp) != font["sha256"]:
        tmp.unlink()
        raise SystemExit(f"SHA-256 mismatch for {font['file']}")
    tmp.replace(path)
    print(f"fetched {font['file']}")

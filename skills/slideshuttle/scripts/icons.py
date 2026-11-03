"""Copy Lucide icons (ISC license) into a deck project, for <Icon src="icons/NAME.svg"/>.

    python3 scripts/icons.py --list chart        # names containing "chart"
    python3 scripts/icons.py lightbulb chart-line --out /path/to/deck/icons

The pinned release is downloaded once, verified by SHA-256 and cached under
~/.cache/slideshuttle. Only the named icons and the license are written to --out.
Browse names at https://lucide.dev/icons.
"""
import argparse
import hashlib
import io
import pathlib
import sys
import tarfile
import urllib.request

VERSION = "1.49.0"
URL = f"https://registry.npmjs.org/lucide-static/-/lucide-static-{VERSION}.tgz"
SHA256 = "c8c9d5dcbc50f388afea623ce2bcbef5788eb0b2d084081aa52d6adaae4ebba3"


def archive():
    cache = pathlib.Path.home() / ".cache" / "slideshuttle" / f"lucide-static-{VERSION}.tgz"
    if not cache.exists() or hashlib.sha256(cache.read_bytes()).hexdigest() != SHA256:
        cache.parent.mkdir(parents=True, exist_ok=True)
        data = urllib.request.urlopen(URL, timeout=60).read()
        if hashlib.sha256(data).hexdigest() != SHA256:
            raise SystemExit("downloaded Lucide archive does not match the pinned SHA-256")
        cache.write_bytes(data)
    return tarfile.open(fileobj=io.BytesIO(cache.read_bytes()), mode="r:gz")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("names", nargs="*", help="icon names, e.g. lightbulb arrow-right")
    parser.add_argument("--out", type=pathlib.Path, help="the deck's icon folder")
    parser.add_argument("--list", metavar="TEXT", nargs="?", const="", help="list icon names containing TEXT")
    args = parser.parse_args()
    with archive() as tar:
        icons = {
            m.name.removeprefix("package/icons/").removesuffix(".svg"): m
            for m in tar.getmembers()
            if m.isfile() and m.name.startswith("package/icons/") and m.name.endswith(".svg")
        }
        if args.list is not None:
            print("\n".join(sorted(n for n in icons if args.list in n)))
            return
        if not args.names or args.out is None:
            parser.error("give icon names and --out, or use --list")
        missing = [n for n in args.names if n not in icons]
        if missing:
            raise SystemExit(f"unknown icons: {', '.join(missing)}; try --list")
        args.out.mkdir(parents=True, exist_ok=True)
        for name in args.names:
            (args.out / f"{name}.svg").write_bytes(tar.extractfile(icons[name]).read())
        (args.out / "LICENSE-lucide.txt").write_bytes(tar.extractfile("package/LICENSE").read())
        print(f"copied {len(args.names)} icons to {args.out}")


if __name__ == "__main__":
    sys.exit(main())

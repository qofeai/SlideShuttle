#!/usr/bin/env python3
"""Measure a complete CLI render in a fresh child (stdlib only; no Office/browser)."""
import argparse
import hashlib
import json
from pathlib import Path
import random
import struct
import subprocess
import sys
import time
import zlib
try:
    import resource
except ImportError:
    resource = None


def png_chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path, help='New directory for inputs, renders and report')
    parser.add_argument('--pages', type=int, default=100)
    parser.add_argument('--steps', action='store_true')
    args = parser.parse_args()
    if not 1 <= args.pages <= 500:
        parser.error('--pages must be in [1,500]')
    cli = args.cli.resolve()
    args.out.mkdir(parents=True, exist_ok=False)
    rng = random.Random(20261103)
    data = b''.join(b'\0' + rng.randbytes(320 * 3) for _ in range(180))
    png = b'\x89PNG\r\n\x1a\n' + png_chunk(b'IHDR', struct.pack('>IIBBBBB', 320, 180, 8, 2, 0, 0, 0)) + png_chunk(b'IDAT', zlib.compress(data)) + png_chunk(b'IEND', b'')
    (args.out / 'noise.png').write_bytes(png)
    slides = ''.join(f'<Slide id="p{i}"><Image id="image" src="noise.png" style="width:640px;height:360px"/><Animate target="image" effect="appear"/></Slide>' for i in range(args.pages))
    source = '<Deck width="640px" height="360px" font-family="DejaVu Sans">' + slides + '</Deck>'
    (args.out / 'deck.xml').write_text(source, encoding='utf-8')
    font = Path(__file__).resolve().parents[1] / 'crates/slide-shuttle-core/tests/fonts/DejaVuSans.ttf'
    command = [str(cli), 'render', str(args.out / 'deck.xml'), '--font', str(font), '--out', str(args.out / 'render')]
    if args.steps:
        command.append('--steps')
    start = time.perf_counter()
    result = subprocess.run(command, capture_output=True, text=True)
    elapsed = time.perf_counter() - start
    (args.out / 'stdout.txt').write_text(result.stdout, encoding='utf-8')
    (args.out / 'stderr.txt').write_text(result.stderr, encoding='utf-8')
    result.check_returncode()
    files = list((args.out / 'render').glob('*.png'))
    rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss if resource else None
    if rss is not None and sys.platform != 'darwin':
        rss *= 1024
    report = {'pages': args.pages, 'steps': args.steps, 'pngCount': len(files), 'seconds': elapsed,
              'peakRssBytes': rss, 'encodedBytes': sum(p.stat().st_size for p in files),
              'firstPngSha256': hashlib.sha256((args.out / 'render/slide-001.png').read_bytes()).hexdigest(),
              'cli': str(cli), 'platform': sys.platform, 'python': sys.version.split()[0]}
    (args.out / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Real CJK/TTC compact-package regression; stdlib only, no font tooling required."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--font', type=Path, action='append', required=True)
    args = parser.parse_args()
    cli = str(args.cli.resolve())
    fonts = [arg for path in args.font for arg in ('--font', str(path.resolve()))]
    repo = Path(__file__).resolve().parents[1]

    def run(*argv):
        result = subprocess.run([cli, *map(str, argv)], capture_output=True, text=True)
        if result.returncode:
            raise AssertionError(f'{argv}: {result.stderr}\n{result.stdout}')
        return json.loads(result.stdout)

    with tempfile.TemporaryDirectory(prefix='compact-qppt-') as directory:
        root = Path(directory)
        (root / 'pages').mkdir()
        (root / 'deck.xml').write_text('<Deck width="640px" height="480px" font-family="Noto Sans CJK SC"><Slide src="pages/vertical.xml"/></Deck>')
        (root / 'pages/vertical.xml').write_text('''<Slide id="vertical">
          <Text id="vertical-text" style="width:180px;height:420px;font-size:28px;line-height:38px;writing-mode:vertical-rl">中文（括号）「竖排」，标点。ABC 123</Text>
          <Text id="fallback" style="position:absolute;left:220px;top:20px;width:380px;font-size:24px;line-height:32px;font-family:'Noto Sans CJK SC', 'Noto Sans CJK JP'">简体与日本語、かな。</Text>
          <Text id="bold" style="position:absolute;left:220px;top:100px;width:380px;font-size:28px;line-height:38px;font-weight:700">粗体与数字 42%</Text>
          <Animate target="bold" effect="appear"/>
        </Slide>''', encoding='utf-8')
        sources = {
            'starter': repo / 'skills/slideshuttle/assets/starter/deck.xml',
            'vertical': root / 'deck.xml',
        }
        report = {}
        for label, source in sources.items():
            full = root / f'{label}-full.qppt'
            compact = root / f'{label}-compact.qppt'
            run('pack', source, '--out', full, '--full-fonts', *fonts)
            packed = run('pack', source, '--out', compact, *fonts)
            assert compact.stat().st_size < full.stat().st_size / 10, packed
            outputs = []
            for mode, package in [('full', full), ('compact', compact)]:
                preview = root / f'{label}-{mode}'
                run('render', package, '--out', preview, '--steps')
                outputs.append({p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in preview.glob('*.png')})
            assert outputs[0] and outputs[0] == outputs[1], f'{label}: rendered pixels changed'
            again = root / f'{label}-again.qppt'
            run('pack', compact, '--out', again)
            assert compact.read_bytes() == again.read_bytes(), f'{label}: repack is not deterministic'
            report[label] = {'fullBytes': full.stat().st_size, 'compactBytes': compact.stat().st_size,
                             'identicalFrames': len(outputs[0]), 'fonts': packed['fonts']}
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()

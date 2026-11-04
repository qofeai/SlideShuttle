"""Build a SlideShuttle delivery, run automated aesthetic/density audits, and leave visual review to the author."""
import argparse
import html
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET

# Presentation aesthetic & density thresholds (1280x720 baseline)
MIN_FONT_PX = 16.0
MIN_BODY_FONT_PX = 20.0
MAX_SLIDE_CHARS = 220
MAX_NODE_LINES = 4
MAX_LIST_LINES = 7
MAX_SLIDE_NODES = 26

FONT_SIZE_RE = re.compile(r'(?:^|;)\s*font-size\s*:\s*([0-9.]+)px\b')
WORD_UNIT_RE = re.compile(r'[\u4e00-\u9fff]|[A-Za-z0-9]+')
MINOR_ID_RE = re.compile(
    r'(kicker|caption|note|source|tag|badge|sub|meta|step|num|label|sample|chart|th|td)',
    re.I,
)
BOILERPLATE_RE = re.compile(
    r'第\s*\d+\s*(?:/\s*\d+\s*)?页|\bpage\s+\d+\s*(?:of\s+\d+|/\s*\d+)\b|\bslide\s+\d+\s*/\s*\d+|由\s*SlideShuttle|Rendered\s+by\s+SlideShuttle',
    re.I,
)


def repository(explicit):
    selected = explicit or os.environ.get('SLIDESHUTTLE_ROOT')
    if selected:
        path = Path(selected).expanduser().resolve()
        if not (path / 'crates/slide-shuttle-cli/Cargo.toml').is_file():
            raise ValueError(f'Not a SlideShuttle repository: {path}')
        return path
    for start in [Path(__file__).resolve().parent, Path.cwd().resolve()]:
        for parent in [start, *start.parents]:
            for candidate in [parent, parent / 'SlideShuttle']:
                if (candidate / 'crates/slide-shuttle-cli/Cargo.toml').is_file():
                    return candidate
    return None


def runtime(args):
    repo = repository(args.repo)
    selected = args.cli or os.environ.get('SLIDESHUTTLE_BIN')
    if selected:
        cli = Path(shutil.which(selected) or selected).expanduser().resolve()
    else:
        suffix = '.exe' if os.name == 'nt' else ''
        candidates = (
            [repo / f'target/{kind}/slide-shuttle{suffix}' for kind in ['release', 'debug']]
            if repo
            else []
        )
        existing = [p for p in candidates if p.is_file() and os.access(p, os.X_OK)]
        cli = max(existing, key=lambda p: p.stat().st_mtime) if existing else None
        if cli is None:
            found = shutil.which('slide-shuttle')
            cli = Path(found).resolve() if found else None
    if cli is None or not cli.is_file() or not os.access(cli, os.X_OK):
        raise ValueError(
            f'SlideShuttle CLI not found. Build with cargo build -p slide-shuttle-cli --locked in {repo or "the repository"}, or pass --cli.'
        )
    fonts = [Path(f).expanduser().resolve() for f in args.font]
    needs_fonts = args.doctor or args.source is None or args.source.suffix.lower() != '.qppt'
    if not fonts and repo and needs_fonts:
        fonts = [repo / f'examples/fonts/NotoSansCJK-{weight}.ttc' for weight in ['Regular', 'Bold']]
    if (needs_fonts and not fonts) or any(not p.is_file() for p in fonts):
        raise ValueError(
            'Provide regular/bold font files with --font, or configure --repo with its bundled Noto CJK fonts.'
        )
    version = subprocess.run(
        [str(cli), '--version'], capture_output=True, text=True, timeout=15, check=True
    ).stdout.strip()
    return {
        'cli': str(cli),
        'version': version,
        'repo': str(repo) if repo else None,
        'fonts': [str(p) for p in fonts],
        'languageDoc': str(repo / 'docs/LANGUAGE.md') if repo else None,
    }


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')


def prepare_out_dir(out):
    """Allow reusing an output directory only if it exclusively holds a prior failed build."""
    if not out.exists():
        out.mkdir(parents=True)
        return
    build_file = out / 'build.json'
    has_artifacts = (out / 'presentation.qppt').exists() or (out / 'presentation.pptx').exists()
    if build_file.is_file() and not has_artifacts:
        try:
            prev = json.loads(build_file.read_text(encoding='utf-8'))
            if prev.get('status') == 'failed':
                shutil.rmtree(out)
                out.mkdir(parents=True)
                return
        except (OSError, json.JSONDecodeError):
            pass
    raise ValueError(f'Output already exists; choose a new revision directory: {out}')


def run_step(info, out, name, arguments):
    command = [info['cli'], *map(str, arguments)]
    process = subprocess.run(command, capture_output=True, text=True, timeout=180)
    try:
        result = json.loads(process.stdout)
    except json.JSONDecodeError:
        result = None
    write_json(
        out / f'{name}.json',
        {
            'command': command,
            'returnCode': process.returncode,
            'result': result,
            'stdout': process.stdout if result is None else None,
            'stderr': process.stderr,
        },
    )
    if process.returncode:
        if isinstance(result, dict) and 'diagnostics' in result:
            detail = json.dumps(
                {
                    'diagnosticCount': len(result['diagnostics']),
                    'diagnostics': result['diagnostics'][:40],
                    'fullReport': str(out / f'{name}.json'),
                },
                ensure_ascii=False,
            )
        else:
            detail = (json.dumps(result, ensure_ascii=False) if result is not None else process.stdout)[:4096]
        raise ValueError(f'{name} failed: {process.stderr.strip()}\n{detail}')
    return result


def extract_font_size(style_str):
    if not style_str:
        return None
    matches = FONT_SIZE_RE.findall(style_str)
    return float(matches[-1]) if matches else None


def count_words(text):
    return len(WORD_UNIT_RE.findall(text or ''))


def visible_text_len(elem, components):
    if elem.tag in ('Notes', 'Animate', 'Font', 'Asset', 'Token', 'Style', 'Component'):
        return 0
    if elem.tag == 'Use':
        comp = components.get(elem.attrib.get('component', ''))
        if comp is None:
            return 0
        raw = ' '.join(comp.itertext())
        for k, v in elem.attrib.items():
            if k not in ('id', 'component'):
                raw = raw.replace(f'${{{k}}}', v)
        return count_words(raw)
    total = count_words(elem.text)
    for child in elem:
        total += visible_text_len(child, components)
        total += count_words(child.tail)
    return total


def audit_design(source, check):
    """Audit slide typography scale, visible text density, line counts, and node clutter."""
    warnings = []
    page_metrics = []
    root_dir = source.parent if source.is_file() else source
    entry = source if source.is_file() else (root_dir / 'deck.xml')
    if not entry.is_file() or source.suffix.lower() == '.qppt':
        return {'passed': True, 'warningCount': 0, 'pageMetrics': [], 'warnings': []}

    styles = {}
    components = {}
    slide_files = []
    try:
        deck_root = ET.parse(entry).getroot()
        for node in deck_root:
            if node.tag == 'Style' and 'name' in node.attrib:
                styles[node.attrib['name']] = (node.text or '').strip()
            elif node.tag == 'Component' and 'name' in node.attrib:
                components[node.attrib['name']] = node
            elif node.tag == 'Library' and 'src' in node.attrib:
                lib_path = root_dir / node.attrib['src']
                if lib_path.is_file():
                    lib_root = ET.parse(lib_path).getroot()
                    for lnode in lib_root:
                        if lnode.tag == 'Style' and 'name' in lnode.attrib:
                            styles[lnode.attrib['name']] = (lnode.text or '').strip()
                        elif lnode.tag == 'Component' and 'name' in lnode.attrib:
                            components[lnode.attrib['name']] = lnode
            elif node.tag == 'Slide' and 'src' in node.attrib:
                slide_files.append(root_dir / node.attrib['src'])
    except ET.ParseError:
        return {'passed': True, 'warningCount': 0, 'pageMetrics': [], 'warnings': []}

    # Check shared styles for overly small fonts
    for name, css in styles.items():
        sz = extract_font_size(css)
        if sz is not None and sz < MIN_FONT_PX:
            warnings.append({
                'code': 'font_too_small',
                'page': 'design.xml',
                'nodeId': f'Style:{name}',
                'message': f'Style "{name}" uses font-size {sz:g}px (< {MIN_FONT_PX:g}px floor). Do not shrink fonts to cram text.',
            })
        elif sz is not None and sz < MIN_BODY_FONT_PX and not MINOR_ID_RE.search(name):
            warnings.append({
                'code': 'body_font_small',
                'page': 'design.xml',
                'nodeId': f'Style:{name}',
                'message': f'Body style "{name}" uses font-size {sz:g}px (recommended >= {MIN_BODY_FONT_PX:g}px for slide readability).',
            })

    check_pages = {p['id']: p for p in (check or {}).get('pages', [])}

    for sfile in slide_files:
        if not sfile.is_file():
            continue
        try:
            sroot = ET.parse(sfile).getroot()
        except ET.ParseError:
            continue
        slide_id = sroot.attrib.get('id', sfile.stem)
        chars = visible_text_len(sroot, components)
        cpage = check_pages.get(slide_id, {'nodes': []})
        nodes = cpage.get('nodes', [])
        node_count = len(nodes)
        min_font = None

        for elem in sroot.iter():
            if elem.tag in ('Text', 'Span', 'Cell', 'Chart', 'Table'):
                nid = elem.attrib.get('id', elem.tag)
                elem_text = ' '.join(elem.itertext())
                if elem.tag == 'Text' and (
                    'footer' in elem.attrib.get('class', '').lower().split()
                    or BOILERPLATE_RE.search(elem_text)
                ):
                    warnings.append({
                        'code': 'unwanted_page_number_or_footer',
                        'page': slide_id,
                        'nodeId': nid,
                        'message': f'Slide "{slide_id}" contains an unrequested page number, footer, or tool watermark ("{elem_text[:40].strip()}"). Remove page numbers and footers unless explicitly requested by the user.',
                    })
                eff_size = None
                for cls in elem.attrib.get('class', '').split():
                    csz = extract_font_size(styles.get(cls, ''))
                    if csz is not None:
                        eff_size = csz
                isz = extract_font_size(elem.attrib.get('style', ''))
                if isz is not None:
                    eff_size = isz
                if eff_size is not None:
                    min_font = eff_size if min_font is None else min(min_font, eff_size)
                    if eff_size < MIN_FONT_PX:
                        warnings.append({
                            'code': 'font_too_small',
                            'page': slide_id,
                            'nodeId': nid,
                            'message': f'Font size {eff_size:g}px is below the {MIN_FONT_PX:g}px minimum ({sfile.name}). Cut text or split the slide.',
                        })
                    elif (
                        elem.tag == 'Text'
                        and eff_size < MIN_BODY_FONT_PX
                        and not MINOR_ID_RE.search(nid)
                        and not MINOR_ID_RE.search(elem.attrib.get('class', ''))
                    ):
                        warnings.append({
                            'code': 'body_font_small',
                            'page': slide_id,
                            'nodeId': nid,
                            'message': f'Text "{nid}" uses {eff_size:g}px (< {MIN_BODY_FONT_PX:g}px body minimum). Prefer short phrases at 20-26px.',
                        })

        if chars > MAX_SLIDE_CHARS:
            warnings.append({
                'code': 'slide_too_dense',
                'page': slide_id,
                'nodeId': slide_id,
                'message': f'Slide "{slide_id}" has {chars} visible characters (limit {MAX_SLIDE_CHARS}, ideal 60-150). Move explanations to <Notes> or split into multiple slides.',
            })

        if node_count > MAX_SLIDE_NODES:
            warnings.append({
                'code': 'visual_clutter',
                'page': slide_id,
                'nodeId': slide_id,
                'message': f'Slide "{slide_id}" has {node_count} rendered items (limit {MAX_SLIDE_NODES}). Remove nested wrapper boxes and keep >= 40% whitespace.',
            })

        max_lines = 0
        for n in nodes:
            lc = n.get('lineCount')
            if lc:
                max_lines = max(max_lines, lc)
                if lc > MAX_LIST_LINES:
                    warnings.append({
                        'code': 'paragraph_too_long',
                        'page': slide_id,
                        'nodeId': n.get('id'),
                        'message': f'Text node "{n.get("id")}" wraps to {lc} lines (limit {MAX_LIST_LINES}). Use concise phrases instead of long paragraphs.',
                    })

        page_metrics.append({
            'id': slide_id,
            'file': sfile.name,
            'visibleChars': chars,
            'nodeCount': node_count,
            'minFontPx': min_font,
            'maxLineCount': max_lines,
        })

    return {
        'passed': len(warnings) == 0,
        'warningCount': len(warnings),
        'pageMetrics': page_metrics,
        'warnings': warnings,
    }


def gallery(out, pages, title):
    images = ''.join(
        f'<figure><a href="preview/{p.name}"><img src="preview/{p.name}" alt="Page {i}" loading="lazy"></a><figcaption>{i:02} / {len(pages):02}</figcaption></figure>'
        for i, p in enumerate(pages, 1)
    )
    (out / 'index.html').write_text(
        f'<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{html.escape(title)}</title><style>body{{margin:24px;background:#edf2f6;color:#142b36;font:16px system-ui}}main{{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr));gap:20px}}figure{{margin:0}}img{{width:100%;background:white}}figcaption{{padding:8px 0}}</style><h1>{html.escape(title)}</h1><main>{images}</main></html>',
        encoding='utf-8',
    )


def contact_sheet(out, pages):
    try:
        from PIL import Image, ImageDraw, ImageOps
    except ImportError:
        return None
    columns = min(3, len(pages))
    rows = math.ceil(len(pages) / columns)
    if columns * 400 * rows * 250 > 64_000_000:
        return None
    sheet = Image.new('RGB', (columns * 400, rows * 250), '#E7EDF1')
    for i, page in enumerate(pages):
        with Image.open(page) as source:
            thumb = ImageOps.contain(source.convert('RGBA'), (392, 220))
        x = (i % columns) * 400 + (400 - thumb.width) // 2
        y = (i // columns) * 250
        sheet.paste(thumb, (x, y), thumb)
        ImageDraw.Draw(sheet).text(((i % columns) * 400 + 8, y + 226), f'{i + 1:02} / {len(pages):02}', fill='#142B36')
    sheet.save(out / 'contact-sheet.png')
    return str(out / 'contact-sheet.png')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path, nargs='?')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--repo')
    parser.add_argument('--cli')
    parser.add_argument('--font', action='append', default=[])
    parser.add_argument('--scale', type=float, default=1)
    parser.add_argument('--strict-design', action='store_true', help='Fail build if aesthetic/density audit reports warnings')
    parser.add_argument('--doctor', action='store_true')
    args = parser.parse_args()
    out = None
    summary = {}
    try:
        info = runtime(args)
        if args.doctor:
            print(json.dumps(info, ensure_ascii=False, indent=2))
            return 0
        if args.source is None or args.out is None:
            parser.error('source and --out are required for a build')
        source = args.source.expanduser().resolve()
        out = args.out.expanduser().resolve()
        if not source.is_file() and not source.is_dir():
            raise ValueError(f'Source does not exist: {source}')
        if not math.isfinite(args.scale) or args.scale <= 0:
            raise ValueError('scale must be positive and finite')
        prepare_out_dir(out)
        summary = {'status': 'building', 'source': str(source), 'runtime': info, 'visualReview': 'pending'}
        fonts = [p for font in info['fonts'] for p in ['--font', font]]
        check = run_step(info, out, 'check', ['check', source, *fonts])
        if not check or not check.get('valid'):
            raise ValueError('check did not return a valid document')
        design_audit = audit_design(source, check)
        if args.strict_design and not design_audit['passed']:
            raise ValueError(f'designAudit failed with {design_audit["warningCount"]} warning(s):\n' + json.dumps(design_audit, ensure_ascii=False, indent=2))
        run_step(info, out, 'pack', ['pack', source, '--out', out / 'presentation.qppt', *fonts])
        run_step(info, out, 'render', ['render', out / 'presentation.qppt', '--out', out / 'preview', '--pdf', '--scale', args.scale])
        run_step(info, out, 'export', ['export', source, '--out', out / 'presentation.pptx', *fonts])
        pages = sorted((out / 'preview').glob('slide-*.png'))
        if len(pages) != len(check['pages']):
            raise ValueError('rendered page count differs from check')
        with (out / 'preview/deck.pdf').open('rb') as f:
            if f.read(5) != b'%PDF-':
                raise ValueError('invalid PDF output')
        with (out / 'presentation.pptx').open('rb') as f:
            if f.read(2) != b'PK':
                raise ValueError('invalid PPTX output')
        gallery(out, pages, source.stem)
        sheet = contact_sheet(out, pages)
        summary.update(
            status='built',
            pages=len(pages),
            qppt=str(out / 'presentation.qppt'),
            pptx=str(out / 'presentation.pptx'),
            pdf=str(out / 'preview/deck.pdf'),
            previews=[str(p) for p in pages],
            gallery=str(out / 'index.html'),
            contactSheet=sheet,
            diagnostics=check['diagnostics'],
            designAudit=design_audit,
        )
        write_json(out / 'build.json', summary)
        print(json.dumps(summary, ensure_ascii=False, indent=2))
        return 0
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        summary.update(status='failed', error=str(error))
        if summary.get('source') and out and out.is_dir():
            write_json(out / 'build.json', summary)
        print(json.dumps(summary, ensure_ascii=False, indent=2), file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())

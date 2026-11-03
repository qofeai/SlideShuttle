"""Build a new SlideShuttle delivery and leave visual judgment to the author."""
import argparse
import html
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys


def repository(explicit):
    selected=explicit or os.environ.get('SLIDESHUTTLE_ROOT')
    if selected:
        path=Path(selected).expanduser().resolve()
        if not (path/'crates/slide-shuttle-cli/Cargo.toml').is_file():
            raise ValueError(f'Not a SlideShuttle repository: {path}')
        return path
    for start in [Path(__file__).resolve().parent,Path.cwd().resolve()]:
        for parent in [start,*start.parents]:
            for candidate in [parent,parent/'SlideShuttle']:
                if (candidate/'crates/slide-shuttle-cli/Cargo.toml').is_file():return candidate
    return None


def runtime(args):
    repo=repository(args.repo)
    selected=args.cli or os.environ.get('SLIDESHUTTLE_BIN')
    if selected:
        cli=Path(shutil.which(selected) or selected).expanduser().resolve()
    else:
        suffix='.exe' if os.name=='nt' else ''
        candidates=[repo/f'target/{kind}/slide-shuttle{suffix}' for kind in ['debug','release']] if repo else []
        cli=next((p for p in candidates if p.is_file() and os.access(p,os.X_OK)),None)
        if cli is None:
            found=shutil.which('slide-shuttle')
            cli=Path(found).resolve() if found else None
    if cli is None or not cli.is_file() or not os.access(cli,os.X_OK):
        raise ValueError(f'SlideShuttle CLI not found. Build with cargo build -p slide-shuttle-cli --locked in {repo or "the repository"}, or pass --cli.')
    fonts=[Path(f).expanduser().resolve() for f in args.font]
    if not fonts and repo:
        fonts=[repo/f'examples/fonts/NotoSansCJK-{weight}.ttc' for weight in ['Regular','Bold']]
    if not fonts or any(not p.is_file() for p in fonts):raise ValueError('Provide regular/bold font files with --font, or configure --repo with its bundled Noto CJK fonts.')
    version=subprocess.run([str(cli),'--version'],capture_output=True,text=True,timeout=15,check=True).stdout.strip()
    return {'cli':str(cli),'version':version,'repo':str(repo) if repo else None,'fonts':[str(p) for p in fonts],'languageDoc':str(repo/'docs/LANGUAGE.md') if repo else None}


def write_json(path,value):
    path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')


def run_step(info,out,name,arguments):
    command=[info['cli'],*map(str,arguments)]
    process=subprocess.run(command,capture_output=True,text=True,timeout=180)
    try:result=json.loads(process.stdout)
    except json.JSONDecodeError:result=None
    write_json(out/f'{name}.json',{'command':command,'returnCode':process.returncode,'result':result,'stdout':process.stdout if result is None else None,'stderr':process.stderr})
    if process.returncode:
        if isinstance(result,dict) and 'diagnostics'in result:
            detail=json.dumps({'diagnosticCount':len(result['diagnostics']),'diagnostics':result['diagnostics'][:40],'fullReport':str(out/f'{name}.json')},ensure_ascii=False)
        else:detail=(json.dumps(result,ensure_ascii=False) if result is not None else process.stdout)[:4096]
        raise ValueError(f'{name} failed: {process.stderr.strip()}\n{detail}')
    return result


def gallery(out,pages,title):
    images=''.join(f'<figure><a href="preview/{p.name}"><img src="preview/{p.name}" alt="Page {i}" loading="lazy"></a><figcaption>{i:02} / {len(pages):02}</figcaption></figure>' for i,p in enumerate(pages,1))
    (out/'index.html').write_text(f'<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{html.escape(title)}</title><style>body{{margin:24px;background:#edf2f6;color:#142b36;font:16px system-ui}}main{{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr));gap:20px}}figure{{margin:0}}img{{width:100%;background:white}}figcaption{{padding:8px 0}}</style><h1>{html.escape(title)}</h1><main>{images}</main></html>',encoding='utf-8')


def contact_sheet(out,pages):
    try:from PIL import Image,ImageDraw,ImageOps
    except ImportError:return None
    columns=min(3,len(pages));rows=math.ceil(len(pages)/columns)
    if columns*400*rows*250>64_000_000:return None
    sheet=Image.new('RGB',(columns*400,rows*250),'#E7EDF1')
    for i,page in enumerate(pages):
        with Image.open(page)as source:thumb=ImageOps.contain(source.convert('RGBA'),(392,220))
        x=(i%columns)*400+(400-thumb.width)//2;y=(i//columns)*250
        sheet.paste(thumb,(x,y),thumb)
        ImageDraw.Draw(sheet).text(((i%columns)*400+8,y+226),f'{i+1:02} / {len(pages):02}',fill='#142B36')
    sheet.save(out/'contact-sheet.png')
    return str(out/'contact-sheet.png')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path,nargs='?')
    parser.add_argument('--out',type=Path)
    parser.add_argument('--repo')
    parser.add_argument('--cli')
    parser.add_argument('--font',action='append',default=[])
    parser.add_argument('--scale',type=float,default=1)
    parser.add_argument('--doctor',action='store_true')
    args=parser.parse_args()
    out=None;summary={}
    try:
        info=runtime(args)
        if args.doctor:print(json.dumps(info,ensure_ascii=False,indent=2));return 0
        if args.source is None or args.out is None:parser.error('source and --out are required for a build')
        source=args.source.expanduser().resolve();out=args.out.expanduser().resolve()
        if not source.is_file():raise ValueError(f'Source does not exist: {source}')
        if out.exists():raise ValueError(f'Output already exists; choose a new revision directory: {out}')
        if not math.isfinite(args.scale) or args.scale<=0:raise ValueError('scale must be positive and finite')
        out.mkdir(parents=True)
        summary={'status':'building','source':str(source),'runtime':info,'visualReview':'pending'}
        fonts=[p for font in info['fonts'] for p in ['--font',font]]
        check=run_step(info,out,'check',['check',source,*fonts])
        if not check or not check.get('valid'):raise ValueError('check did not return a valid document')
        run_step(info,out,'render',['render',source,'--out',out/'preview','--pdf','--scale',args.scale,*fonts])
        run_step(info,out,'export',['export',source,'--out',out/'presentation.pptx',*fonts])
        pages=sorted((out/'preview').glob('slide-*.png'))
        if len(pages)!=len(check['pages']):raise ValueError('rendered page count differs from check')
        with (out/'preview/deck.pdf').open('rb')as f:
            if f.read(5)!=b'%PDF-':raise ValueError('invalid PDF output')
        with (out/'presentation.pptx').open('rb')as f:
            if f.read(2)!=b'PK':raise ValueError('invalid PPTX output')
        gallery(out,pages,source.stem)
        sheet=contact_sheet(out,pages)
        summary.update(status='built',pages=len(pages),pptx=str(out/'presentation.pptx'),pdf=str(out/'preview/deck.pdf'),previews=[str(p) for p in pages],gallery=str(out/'index.html'),contactSheet=sheet,diagnostics=check['diagnostics'])
        write_json(out/'build.json',summary)
        print(json.dumps(summary,ensure_ascii=False,indent=2));return 0
    except (ValueError,OSError,subprocess.SubprocessError)as error:
        summary.update(status='failed',error=str(error))
        if summary.get('source') and out and out.is_dir():write_json(out/'build.json',summary)
        print(json.dumps(summary,ensure_ascii=False,indent=2),file=sys.stderr);return 1


if __name__=='__main__':sys.exit(main())

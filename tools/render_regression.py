"""Rerender the frozen benchmark source; no model regeneration or source repair."""
import argparse
import hashlib
import json
import re
import subprocess
import unicodedata
import xml.etree.ElementTree as ET
from pathlib import Path
from project_source import write_project


def words(text):return {w.casefold()for w in re.findall(r"\w+",unicodedata.normalize("NFKC",text))if len(w)>=4}


def main():
    p=argparse.ArgumentParser()
    p.add_argument("--source",type=Path,default=Path("benchmarks/runs/2026-11-02-slidesbench-pilot/attempt-2/raw-deck.xml"))
    p.add_argument("--out",type=Path,default=Path("examples/output/rendering-v2"))
    p.add_argument("--container",default="slideshuttle-rendering-v2")
    p.add_argument("--font",type=Path,action="append",required=True)
    a=p.parse_args();root=Path.cwd().resolve();out=a.out.resolve()
    if out.exists():raise ValueError("choose a new regression output directory")
    out.mkdir(parents=True)
    font_args=[part for f in a.font for part in["--font",str(f.resolve())]]
    source=a.source.resolve();binary=root/"target/debug/slide-shuttle"
    def run(args,name):
        process=subprocess.run(args,capture_output=True,text=True,timeout=180)
        (out/f"{name}.json").write_text(json.dumps({"argv":[str(x)for x in args],"returnCode":process.returncode,"stdout":process.stdout,"stderr":process.stderr},ensure_ascii=False,indent=2))
        if process.returncode:raise RuntimeError(process.stderr)
        return process.stdout
    project_entry=write_project(ET.parse(source).getroot(),out/"source.xml")
    run([str(binary),"check",str(project_entry),*font_args],"check")
    run([str(binary),"render",str(project_entry),"--out",str(out/"native"),"--pdf",*font_args],"render")
    run([str(binary),"export",str(project_entry),"--out",str(out/"SlideShuttle-benchmark-v2.pptx"),*font_args],"export")
    office=out/"libreoffice";office.mkdir()
    in_container=Path("/work")/out.relative_to(root)
    run(["docker","exec",a.container,"libreoffice","-env:UserInstallation=file:///tmp/render-regression-v2","--headless","--convert-to","pdf","--outdir",str(in_container/"libreoffice"),str(in_container/"SlideShuttle-benchmark-v2.pptx")],"libreoffice")
    import pymupdf
    from PIL import Image,ImageDraw,ImageOps
    slides=ET.parse(source).getroot().findall("Slide")
    records=[];thumbs=[]
    with pymupdf.open(office/"SlideShuttle-benchmark-v2.pdf")as document:
        if len(document)!=len(slides):raise ValueError("page count mismatch")
        for i,(page,slide)in enumerate(zip(document,slides)):
            png=out/f"slide-{i+1:03}.png";page.get_pixmap(matrix=pymupdf.Matrix(1280/page.rect.width,720/page.rect.height)).save(png)
            expected=" ".join("".join(t.itertext())for t in slide.iter("Text"));rendered=page.get_text();(out/f"slide-{i+1:03}.txt").write_text(rendered)
            record={"id":slide.attrib["id"],"sourceWords4Plus":len(words(expected)),"missingOrSplitWords":sorted(words(expected)-words(rendered))};records.append(record)
            tile=Image.new("RGB",(400,250),"white");tile.paste(ImageOps.contain(Image.open(png),(400,225)),(0,0));ImageDraw.Draw(tile).text((8,230),record["id"],fill="black");thumbs.append(tile)
    sheet=Image.new("RGB",(1200,250*((len(thumbs)+2)//3)),"#E2E8F0")
    for i,tile in enumerate(thumbs):sheet.paste(tile,((i%3)*400,(i//3)*250))
    sheet.save(out/"contact-sheet.png")
    report={"sourceSha256":hashlib.sha256(source.read_bytes()).hexdigest(),"engineVersion":subprocess.check_output([str(binary),"--version"],text=True).strip(),"renderer":subprocess.check_output(["docker","exec",a.container,"libreoffice","--version"],text=True).strip(),"pages":len(records),"pagesWithMissingOrSplitWords":sum(bool(r["missingOrSplitWords"])for r in records),"sourceRegenerated":False,"cases":records}
    (out/"report.json").write_text(json.dumps(report,ensure_ascii=False,indent=2));print(json.dumps(report,ensure_ascii=False,indent=2))
    if any(r["missingOrSplitWords"]for r in records):raise SystemExit(2)


if __name__=="__main__":main()

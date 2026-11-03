"""Execute a saved SlidesBench XML response and retain successes AND failures.

This is an adapted, image-free pilot. It never substitutes local checks for an
official SlidesBench score. Optional LibreOffice output is an independent render.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import time
import unicodedata
import xml.etree.ElementTree as ET
from pathlib import Path


def normal(text):
    text = unicodedata.normalize("NFKC", text).casefold()
    return " ".join(re.findall(r"\w+", text))


def extract_deck(response):
    start = response.find("<Deck")
    end = response.rfind("</Deck>")
    if start < 0 or end < start:
        raise ValueError("response has no complete Deck; incomplete output remains a failed run")
    return response[start:end + len("</Deck>")]


def execute(command, out, timeout=120, env=None):
    start = time.monotonic()
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=timeout, env=env)
        record = {"argv": [str(v) for v in command], "returnCode": result.returncode, "stdout": result.stdout, "stderr": result.stderr, "seconds": time.monotonic() - start}
    except subprocess.TimeoutExpired as e:
        record = {"argv": [str(v) for v in command], "returnCode": None, "stdout": str(e.stdout or ""), "stderr": "timeout", "seconds": time.monotonic() - start}
    out.write_text(json.dumps(record, ensure_ascii=False, indent=2))
    return record


def quoted_requirements(instruction):
    # Diagnostic only: no judge and no paraphrase scoring. Store every missed span.
    return list(dict.fromkeys(s.strip() for s in re.findall(r'["“]([^"”\n]+)["”]', instruction) if normal(s)))


def potential_overlaps(nodes):
    text = [n for n in nodes if n.get("lineCount") is not None]
    pairs = []
    for i, a in enumerate(text):
        for b in text[i+1:]:
            x, y = a["rect"], b["rect"]
            width = min(x["x"]+x["width"],y["x"]+y["width"])-max(x["x"],y["x"])
            height = min(x["y"]+x["height"],y["y"]+y["height"])-max(x["y"],y["y"])
            area = min(x["width"]*x["height"],y["width"]*y["height"])
            if width > 2 and height > 2 and area > 0 and width*height/area > .03:
                pairs.append({"a": a["id"], "b": b["id"], "smallerBoxFraction": width*height/area})
    return pairs


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--run", type=Path, required=True)
    p.add_argument("--attempt", default="attempt-1")
    p.add_argument("--binary", type=Path, default=Path("target/debug/slide-shuttle"))
    p.add_argument("--font", type=Path, action="append", required=True)
    p.add_argument("--libreoffice-root", type=Path)
    args = p.parse_args()
    run = args.run.resolve()
    attempt = run / args.attempt
    tasks = json.loads((run / "tasks.json").read_text())
    xml = extract_deck((attempt / "response.md").read_text())
    (attempt / "raw-deck.xml").write_text(xml)
    deck = ET.fromstring(xml)
    slides = deck.findall("Slide")
    expected = [t["id"] for t in tasks["tasks"]]
    actual = [s.get("id") for s in slides]
    if actual != expected:
        raise ValueError(f"task/slide identity mismatch: {actual}")
    reports = []
    binary = str(args.binary.resolve())
    font_args = [value for font in args.font for value in ("--font", str(font.resolve()))]
    for task, slide in zip(tasks["tasks"], slides):
        case = attempt / task["id"]
        case.mkdir(exist_ok=True)
        doc = ET.Element("Deck", deck.attrib)
        for declaration in deck:
            if declaration.tag != "Slide":
                doc.append(declaration)
        doc.append(slide)
        source = case / "source.xml"
        source.write_text(ET.tostring(doc, encoding="unicode"))
        checked = execute([binary,"check",str(source),*font_args], case/"check-command.json")
        try:
            check = json.loads(checked["stdout"])
        except json.JSONDecodeError:
            check = None
        required = quoted_requirements(task["instruction"])
        text = " ".join(slide.itertext())
        # Text values appear only in text nodes; CSS declarations are not counted.
        text = " ".join("".join(n.itertext()) for n in slide.iter() if n.tag == "Text")
        missed = [s for s in required if normal(s) not in normal(text)]
        row = {"id": task["id"], "domain": task["domain"], "checkPassed": checked["returnCode"]==0, "checkSeconds": checked["seconds"], "diagnostics": check.get("diagnostics",[]) if check else [], "checkError": checked["stderr"], "quotedContent": {"required":len(required),"found":len(required)-len(missed),"missing":missed}, "potentialTextBoxOverlaps": potential_overlaps(check["pages"][0]["nodes"]) if check else []}
        if row["checkPassed"]:
            rendered = execute([binary,"render",str(source),"--out",str(case/"native"),"--pdf",*font_args],case/"render-command.json")
            exported = execute([binary,"export",str(source),"--out",str(case/"output.pptx"),*font_args],case/"export-command.json")
            row.update(renderPassed=rendered["returnCode"]==0,exportPassed=exported["returnCode"]==0,renderSeconds=rendered["seconds"],exportSeconds=exported["seconds"])
            if row["exportPassed"]:
                from pptx import Presentation
                presentation = Presentation(case/"output.pptx")
                row["pptxPages"] = len(presentation.slides)
                row["nativeTextObjects"] = sum(s.has_text_frame and bool(s.text_frame.text.strip()) for s in presentation.slides[0].shapes)
                sizes=[r.font.size.pt for s in presentation.slides[0].shapes if s.has_text_frame for p in s.text_frame.paragraphs for r in p.runs if r.font.size]
                row["minExplicitFontPt"] = min(sizes) if sizes else None
                if args.libreoffice_root:
                    prefix = args.libreoffice_root.resolve()
                    program = prefix/"usr/lib/libreoffice/program"
                    lo_out = case/"libreoffice"
                    lo_out.mkdir(exist_ok=True)
                    env=dict(os.environ, LD_LIBRARY_PATH=f"{prefix}/usr/lib/x86_64-linux-gnu:{program}",SAL_USE_VCLPLUGIN="svp")
                    command=[str(program/"soffice"),f"-env:UserInstallation={(case/'lo-profile').as_uri()}","--headless","--convert-to","pdf","--outdir",str(lo_out),str(case/"output.pptx")]
                    lo=execute(command,case/"libreoffice-command.json",env=env)
                    pdf=lo_out/"output.pdf"
                    row["libreOfficePassed"] = lo["returnCode"]==0 and pdf.exists()
                    if row["libreOfficePassed"]:
                        import pymupdf
                        with pymupdf.open(pdf) as document:
                            page=document[0]
                            page.get_pixmap(matrix=pymupdf.Matrix(1280/page.rect.width,720/page.rect.height)).save(lo_out/"slide-001.png")
                            (lo_out/"text.txt").write_text(page.get_text())
                            lo_missed=[s for s in required if normal(s)not in normal(page.get_text())]
                            row["libreOfficeQuotedContent"]={"required":len(required),"found":len(required)-len(lo_missed),"missing":lo_missed}
        reports.append(row)
        print(json.dumps({k:row.get(k)for k in ["id","checkPassed","renderPassed","exportPassed","libreOfficePassed"]}),flush=True)
    summary={"benchmark":"SlidesBench adapted image-free pilot","attempt":args.attempt,"inputSha256":hashlib.sha256(xml.encode()).hexdigest(),"total":len(reports),"checkPassed":sum(r["checkPassed"]for r in reports),"renderPassed":sum(r.get("renderPassed",False)for r in reports),"exportPassed":sum(r.get("exportPassed",False)for r in reports),"libreOfficePassed":sum(r.get("libreOfficePassed",False)for r in reports),"quotedRequired":sum(r["quotedContent"]["required"]for r in reports),"quotedFound":sum(r["quotedContent"]["found"]for r in reports),"cases":reports,"officialLeaderboardScore":None}
    (attempt/"results.json").write_text(json.dumps(summary,ensure_ascii=False,indent=2))
    print(json.dumps({k:v for k,v in summary.items()if k!="cases"},indent=2))


if __name__ == "__main__":
    main()

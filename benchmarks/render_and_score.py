"""Independent PPTX render + unmodified upstream SlidesGen aesthetic metrics.

Text detection is adapted: exact PDF text boxes replace PP-DocLayout_plus-L.
No deck rhythm/overall leaderboard score is manufactured from unrelated tasks.
"""
import argparse
import hashlib
import json
import os
import statistics
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

from evaluate_pilot import normal, quoted_requirements


def structure(path):
    namespaces={"p":"http://schemas.openxmlformats.org/presentationml/2006/main","a":"http://schemas.openxmlformats.org/drawingml/2006/main"}
    with zipfile.ZipFile(path)as z:
        masters=[n for n in z.namelist()if n.startswith("ppt/slideMasters/")and n.endswith(".xml")]
        inherited_objects=0
        for name in masters:
            doc=ET.fromstring(z.read(name));tree=doc.find("p:cSld/p:spTree",namespaces)
            if tree is not None:inherited_objects+=sum(n.tag.rsplit("}",1)[-1]in["sp","pic","graphicFrame","grpSp","cxnSp"]for n in tree)
        slides=[ET.fromstring(z.read(n))for n in z.namelist()if n.startswith("ppt/slides/slide")and n.endswith(".xml")]
        groups=sum(len(d.findall(".//p:grpSp",namespaces))for d in slides)
        charts=[n for n in z.namelist()if n.startswith("ppt/charts/")and n.endswith(".xml")]
        workbooks=[n for n in z.namelist()if n.startswith("ppt/embeddings/")and n.endswith(".xlsx")]
        text_count=sum(len(d.findall(".//a:t",namespaces))for d in slides)
        return {"textRuns":text_count,"masters":len(masters),"masterObjects":inherited_objects,"nativeGroups":groups,"nativeCharts":len(charts),"embeddedWorkbooks":len(workbooks),"peiL3MasterInheritanceVerified":inherited_objects>0,"peiUpperBound":2 if inherited_objects==0 else None,"peiAssessment":"Local audit against published PEI knockout criteria; not an official PEI evaluator. Blank master alone does not establish L3."}


def main():
    p=argparse.ArgumentParser()
    p.add_argument("--run",type=Path,required=True)
    p.add_argument("--attempt",default="attempt-1")
    p.add_argument("--container",default="slideshuttle-benchmark-renderer")
    p.add_argument("--upstream",type=Path,default=Path("benchmarks/cache/upstream/slidesgen"))
    args=p.parse_args();root=Path.cwd().resolve();run=args.run.resolve();attempt=run/args.attempt
    tasks=json.loads((run/"tasks.json").read_text())["tasks"]
    results=json.loads((attempt/"results.json").read_text());upstream=args.upstream.resolve()
    sys.path.insert(0,str(upstream/"eval"))
    import aesthetics_metrics as metrics
    from PIL import Image,ImageOps,ImageDraw
    import pymupdf
    metrics.load_parameters_from_config(str(upstream/"eval/aesthetics_config.json"))
    renderer=subprocess.check_output(["docker","exec",args.container,"libreoffice","--version"],text=True).strip()
    scored=[];thumbnails=[]
    for task,row in zip(tasks,results["cases"]):
        case=attempt/task["id"]
        if not row.get("exportPassed"):
            thumb=Image.new("RGB",(400,250),"#FFF1F2");ImageDraw.Draw(thumb).text((18,18),task["id"]+"\nFIRST-PASS LAYOUT ERROR",fill="#9F1239");thumbnails.append(thumb);continue
        directory=case/"libreoffice";directory.mkdir(exist_ok=True)
        work_case=Path("/work")/case.relative_to(root)
        cmd=["docker","exec",args.container,"libreoffice",f"-env:UserInstallation=file:///tmp/lo-{args.attempt}-{task['id']}","--headless","--convert-to","pdf","--outdir",str(work_case/"libreoffice"),str(work_case/"output.pptx")]
        start=time.monotonic();r=subprocess.run(cmd,capture_output=True,text=True,timeout=90)
        (case/"libreoffice-command.json").write_text(json.dumps({"argv":cmd,"returnCode":r.returncode,"stdout":r.stdout,"stderr":r.stderr,"seconds":time.monotonic()-start},indent=2))
        pdf=directory/"output.pdf"
        if r.returncode or not pdf.exists():
            scored.append({"id":task["id"],"renderError":r.stderr+r.stdout});continue
        with pymupdf.open(pdf)as doc:
            page=doc[0];scale=1280/page.rect.width
            png=directory/"slide-001.png";page.get_pixmap(matrix=pymupdf.Matrix(scale,scale)).save(png)
            text=page.get_text();(directory/"text.txt").write_text(text)
            boxes=[]
            for block in page.get_text("dict")["blocks"]:
                for line in block.get("lines",[]):
                    for span in line.get("spans",[]):
                        if span["text"].strip():boxes.append({"label":"text","coordinate":[max(0,v*scale+(-1 if i<2 else 1))for i,v in enumerate(span["bbox"])]})
        (directory/"text-boxes.json").write_text(json.dumps({"source":"PyMuPDF exact PDF span bounds; adapted detector","boxes":boxes},indent=2))
        image=Image.open(png).convert("RGB")
        raw=metrics.SlideAestheticsCalculator().calculate_slide_metrics(str(png),metrics_to_compute=["color_harmony","colorfulness","subband_entropy"])
        raw["figure_ground_contrast"]=metrics.FigureGroundContrastMetric.execute(image,detection_boxes=boxes)
        required=quoted_requirements(task["instruction"]);missed=[s for s in required if normal(s)not in normal(text)]
        report={"id":task["id"],"renderer":renderer,"imageSha256":hashlib.sha256(png.read_bytes()).hexdigest(),"quotedRequired":len(required),"quotedFound":len(required)-len(missed),"quotedMissing":missed,"metrics":raw,"structure":structure(case/"output.pptx")}
        (case/"aesthetics.json").write_text(json.dumps(report,ensure_ascii=False,indent=2));scored.append(report)
        thumb=Image.new("RGB",(400,250),"white");thumb.paste(ImageOps.contain(image,(400,225)),(0,0));ImageDraw.Draw(thumb).text((8,230),task["id"],fill="black");thumbnails.append(thumb)
        print(json.dumps({"id":task["id"],"quotedFound":report["quotedFound"],"quotedRequired":len(required),"textContrast":raw["figure_ground_contrast"]}),flush=True)
    if thumbnails:
        contact=Image.new("RGB",(400*3,250*((len(thumbnails)+2)//3)),"#E2E8F0")
        for i,thumb in enumerate(thumbnails):contact.paste(thumb,((i%3)*400,(i//3)*250))
        contact.save(attempt/"contact-sheet.png")
    valid=[r for r in scored if "metrics"in r]
    summary={"renderer":renderer,"upstreamCommit":json.loads((upstream/"tree.json").read_text())["sha"],"upstreamCodeSha256":hashlib.sha256((upstream/"eval/aesthetics_metrics.py").read_bytes()).hexdigest(),"upstreamConfigSha256":hashlib.sha256((upstream/"eval/aesthetics_config.json").read_bytes()).hexdigest(),"attempt":args.attempt,"totalTasks":len(tasks),"renderedAndScored":len(valid),"quotedDeliveredFound":sum(r["quotedFound"]for r in valid),"quotedRequiredAllTasks":results["quotedRequired"],"officialLeaderboardScore":None,"adaptations":["12 image-free single-slide tasks; not full SlidesBench test set","Different generation model: interactive Claude; one repair allowance tracked separately","Exact PDF span boxes replace Paddle layout detector","Unrelated tasks do not form a narrative deck: no deck rhythm or overall aesthetics score","Scores on successful outputs are survivor-conditional; failures retained in total task denominator"],"averagesOnSuccessfulOutputs":{},"cases":scored}
    if valid:
        summary["averagesOnSuccessfulOutputs"]={"normalizedTextContrast":statistics.mean(r["metrics"]["figure_ground_contrast"]["mean"]for r in valid),"colorHarmonyDistance":statistics.mean(r["metrics"]["color_harmony"]["best_distance"]for r in valid),"colorfulnessRaw":statistics.mean(r["metrics"]["colorfulness"]for r in valid),"subbandEntropy":statistics.mean(r["metrics"]["subband_entropy"]["subband_entropy"]for r in valid)}
    (attempt/"scored-results.json").write_text(json.dumps(summary,ensure_ascii=False,indent=2))
    print(json.dumps({k:v for k,v in summary.items()if k!="cases"},indent=2))


if __name__=="__main__":main()

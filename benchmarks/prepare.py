"""Fetch pinned public benchmark inputs; source data stays in ignored cache/runs."""
import argparse
import hashlib
import json
import urllib.request
from pathlib import Path

SLIDESGEN_COMMIT="b202e60c39e19c5f9da57029fe53ff189e6642b0"


def fetch(url,path,expected=None):
    path.parent.mkdir(parents=True,exist_ok=True)
    if not path.exists():
        with urllib.request.urlopen(url,timeout=60)as response:data=response.read(16*1024*1024+1)
        if len(data)>16*1024*1024:raise ValueError("benchmark file exceeds download budget")
        path.write_bytes(data)
    data=path.read_bytes()
    if expected and hashlib.sha256(data).hexdigest()!=expected:raise ValueError(f"source hash mismatch: {path}")
    return data


def main():
    p=argparse.ArgumentParser();p.add_argument("--run",type=Path,required=True);args=p.parse_args()
    manifest=json.loads(Path("benchmarks/slidesbench-pilot-manifest.json").read_text());tasks=[]
    for row in manifest["tasks"]:
        data=fetch(f"https://raw.githubusercontent.com/para-lost/AutoPresent/{manifest['upstreamCommit']}/{row['upstreamPath']}",Path("benchmarks/cache/upstream/autopresent")/row['upstreamPath'],row['sha256'])
        tasks.append({**row,"instruction":data.decode()})
    args.run.mkdir(parents=True,exist_ok=True)
    output=args.run/"tasks.json"
    if output.exists():raise ValueError("task file already exists; choose a new run directory")
    output.write_text(json.dumps({**manifest,"tasks":tasks},ensure_ascii=False,indent=2))
    prompt=Path("benchmarks/generation-template.md").read_text()+"\nLANGUAGE CONTRACT\n"+Path("docs/LANGUAGE.md").read_text()+"\nTASKS\n"+json.dumps(tasks,ensure_ascii=False,indent=2)
    (args.run/"generation-prompt.md").write_text(prompt)
    upstream=Path("benchmarks/cache/upstream/slidesgen")
    files=["eval/aesthetics_metrics.py","eval/aesthetics_config.json","docs/pei.md","README.md"]
    hashes={}
    for file in files:
        data=fetch(f"https://raw.githubusercontent.com/YunqiaoYang/SlidesGen-Bench/{SLIDESGEN_COMMIT}/{file}",upstream/file);hashes[file]=hashlib.sha256(data).hexdigest()
    # The scorer only requires the pinned commit ID from this metadata file.
    if not(upstream/"tree.json").exists():(upstream/"tree.json").write_text(json.dumps({"sha":SLIDESGEN_COMMIT}))
    (args.run/"upstream-hashes.json").write_text(json.dumps(hashes,indent=2))
    print(f"Prepared {len(tasks)} verified public task instructions at {output}")


if __name__=="__main__":main()

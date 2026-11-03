"""Measure the owned theme fixture after rendering it in an independent Office engine."""
import argparse
import json
from pathlib import Path

import pymupdf
from PIL import Image


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pdf',type=Path,required=True)
    parser.add_argument('--native',type=Path,required=True)
    parser.add_argument('--out',type=Path,required=True)
    args=parser.parse_args();args.out.mkdir(parents=True,exist_ok=True)
    colors=[];text=[]
    with pymupdf.open(args.pdf)as document:
        assert len(document)==3
        for index,page in enumerate(document):
            png=args.out/f'slide-{index+1:03}.png'
            page.get_pixmap(matrix=pymupdf.Matrix(1280/page.rect.width,720/page.rect.height)).save(png)
            actual=Image.open(png).convert('RGB')
            expected=Image.open(args.native/f'slide-{index+1:03}.png').convert('RGB')
            points=[(20,20)]+([(200,320),(600,320),(1000,320)]if index!=1 else [])
            for point in points:
                a=actual.getpixel(point);b=expected.getpixel(point)
                colors.append({'page':index+1,'point':point,'office':a,'native':b,'maxDifference':max(abs(x-y)for x,y in zip(a,b))})
            spans=[s for block in page.get_text('dict')['blocks']for line in block.get('lines',[])for s in line['spans']]
            assert abs(spans[0]['size']-30)<.02 and spans[0]['font'].endswith('Bold'),spans
            if index==1:
                body=next(s for s in spans if '32px'in s['text'])
                assert abs(body['size']-24)<.02 and body['color']==0xFFFFFF and body['font'].endswith('Regular'),body
            text.extend({'page':index+1,'text':s['text'],'sizePt':s['size'],'font':s['font'],'color':s['color']}for s in spans)
    report={'pages':3,'colorsCompared':len(colors),'maxColorChannelDifference':max(c['maxDifference']for c in colors),'allWithinOneChannelStep':all(c['maxDifference']<=1 for c in colors),'declaredTextSizesAndStylesVerified':True,'colors':colors,'text':text}
    (args.out/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2))
    print(json.dumps({k:v for k,v in report.items()if k not in ['colors','text']},indent=2))


if __name__=='__main__':main()

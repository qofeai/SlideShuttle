"""Build an owned theme/alpha/default-style PPTX and an explicit-color XML oracle."""
import argparse
import xml.etree.ElementTree as ET
from pathlib import Path
from zipfile import ZipFile
from project_source import write_project

A='http://schemas.openxmlformats.org/drawingml/2006/main'
P='http://schemas.openxmlformats.org/presentationml/2006/main'
for prefix,ns in [('a',A),('p',P)]:ET.register_namespace(prefix,ns)

def color(value,transforms=''):
    return f'<a:solidFill><a:schemeClr val="{value}">{transforms}</a:schemeClr></a:solidFill>'

def shape(ident,x,y,w,h,fill='',body='',style='',placeholder=False,kind='rect'):
    return f'''<p:sp><p:nvSpPr><p:cNvPr id="{ident}" name="Fixture {ident}"/><p:cNvSpPr/><p:nvPr>{'<p:ph type="body" idx="7"/>' if placeholder else ''}</p:nvPr></p:nvSpPr>
      <p:spPr><a:xfrm><a:off x="{round(x*9525)}" y="{round(y*9525)}"/><a:ext cx="{round(w*9525)}" cy="{round(h*9525)}"/></a:xfrm><a:prstGeom prst="{kind}"><a:avLst/></a:prstGeom>{fill}</p:spPr>{style}{body}</p:sp>'''

def text(ident,x,y,value,size=None,ink=None,bold=None,placeholder=False,line=None):
    attrs=''.join(f' {k}="{v}"' for k,v in [('sz',size),('b',None if bold is None else int(bold))] if v is not None)
    ppr=f'<a:pPr><a:lnSpc><a:spcPts val="{line}"/></a:lnSpc></a:pPr>' if line else ''
    body=f'<p:txBody><a:bodyPr lIns="0" rIns="0" tIns="0" bIns="0"/><a:lstStyle/><a:p>{ppr}<a:r><a:rPr{attrs}>{color(ink) if ink else ""}</a:rPr><a:t>{value}</a:t></a:r></a:p></p:txBody>'
    return shape(ident,x,y,1168,100,body=body,placeholder=placeholder)

def expected_text(ident,x,y,value,size=40,ink='#142B36',bold=700,line=56):
    return f'<Text id="{ident}" style="position:absolute;left:{x}px;top:{y}px;width:1168px;height:100px;font-size:{size}px;line-height:{line}px;font-weight:{bold};color:{ink}">{value}</Text>'

def page(objects,background):
    return f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld>{background}<p:spTree><p:nvGrpSpPr/><p:grpSpPr/>{objects}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>'

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--base',type=Path,required=True);p.add_argument('--out',type=Path,required=True);p.add_argument('--expected',type=Path,required=True);args=p.parse_args()
    if args.base.resolve()==args.out.resolve():raise ValueError('fixture must not replace the input deck')
    with ZipFile(args.base)as z:parts={info.filename:(info,z.read(info))for info in z.infolist()}
    for name in ['ppt/slides/slide1.xml','ppt/slides/slide2.xml','ppt/slides/slide3.xml']:
        if name not in parts:raise ValueError('base fixture must contain three slides')
    def put(path,xml):parts[path]=(parts[path][0],xml.encode())
    theme=ET.fromstring(parts['ppt/theme/theme1.xml'][1]);scheme=theme.find(f'./{{{A}}}themeElements/{{{A}}}clrScheme')
    scheme.clear();scheme.set('name','Theme regression')
    palette={'dk1':'142B36','lt1':'FFFFFF','dk2':'404040','lt2':'EFF5F3','accent1':'087E6D','accent2':'1D4ED8','accent3':'F59E0B','accent4':'123456','accent5':'8B5CF6','accent6':'EF4444','hlink':'2563EB','folHlink':'7C3AED'}
    for key,value in palette.items():
        entry=ET.SubElement(scheme,f'{{{A}}}{key}');ET.SubElement(entry,f'{{{A}}}'+('sysClr' if key=='accent4' else 'srgbClr'),{'val':'windowText','lastClr':value} if key=='accent4' else {'val':value})
    for family in theme.findall(f'.//{{{A}}}fontScheme/*/*'):
        if family.tag.rsplit('}',1)[-1]in ['latin','ea','cs']:family.set('typeface','Noto Sans CJK SC')
    put('ppt/theme/theme1.xml',ET.tostring(theme,encoding='unicode'))
    presentation=ET.fromstring(parts['ppt/presentation.xml'][1]);old=presentation.find(f'{{{P}}}defaultTextStyle')
    if old is not None:presentation.remove(old)
    presentation.append(ET.fromstring(f'<p:defaultTextStyle xmlns:p="{P}" xmlns:a="{A}"><a:lvl1pPr><a:lnSpc><a:spcPts val="4200"/></a:lnSpc><a:defRPr sz="3000" b="1" i="0">{color("dk1")}<a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle>'))
    put('ppt/presentation.xml',ET.tostring(presentation,encoding='unicode'))
    layout=ET.fromstring(parts['ppt/slideLayouts/slideLayout1.xml'][1]);tree=layout.find(f'./{{{P}}}cSld/{{{P}}}spTree')
    placeholder=text(1000,56,220,'',size=2400,ink='lt1',bold=False,placeholder=True,line=3600)
    placeholder=ET.fromstring(f'<root xmlns:p="{P}" xmlns:a="{A}">{placeholder}</root>')[0]
    paragraph=placeholder.find(f'.//{{{A}}}p');run=paragraph.find(f'{{{A}}}r');rpr=run.find(f'{{{A}}}rPr');rpr.tag=f'{{{A}}}defRPr';paragraph.find(f'{{{A}}}pPr').append(rpr);paragraph.remove(run)
    level=ET.fromstring(ET.tostring(paragraph.find(f'{{{A}}}pPr')));level.tag=f'{{{A}}}lvl1pPr';placeholder.find(f'.//{{{A}}}lstStyle').append(level)
    tree.append(placeholder);put('ppt/slideLayouts/slideLayout1.xml',ET.tostring(layout,encoding='unicode'))
    expected=[]
    objects=text(2,56,48,'主题配色、透明度与默认文字样式')
    cases=[('accent1','', '#087E6D'),('accent2','<a:alpha val="50000"/>','#1D4ED880'),('accent3','<a:tint val="50000"/>','#FAD6BC')]
    xml=expected_text('one/title',56,48,'主题配色、透明度与默认文字样式')
    for i,(name,transform,resolved)in enumerate(cases):
        x=56+394*i;objects+=shape(3+i,x,220,340,240,color(name,transform));xml+=f'<Shape id="one/card-{i}" style="position:absolute;left:{x}px;top:220px;width:340px;height:240px;background:{resolved}"/>'
    objects+=text(6,56,540,'三块色卡分别验证主题色、半透明叠色、主题色变浅。',1800,bold=False,line=2400)
    xml+=expected_text('one/note',56,540,'三块色卡分别验证主题色、半透明叠色、主题色变浅。',24,bold=400,line=32)
    put('ppt/slides/slide1.xml',page(objects,f'<p:bg><p:bgPr>{color("lt2")}</p:bgPr></p:bg>'));expected.append(f'<Slide id="one" background="#EFF5F3">{xml}</Slide>')
    objects=text(2,56,48,'母版、版式与局部文字继承',ink='lt1')+text(3,56,220,'这行文字继承版式的 32px 常规字体。',placeholder=True)+text(4,56,540,'默认样式提供字体，局部设置可以关闭粗体。',1800,'lt1',False,line=2400)
    xml=expected_text('two/title',56,48,'母版、版式与局部文字继承',ink='#FFFFFF')+expected_text('two/body',56,220,'这行文字继承版式的 32px 常规字体。',32,'#FFFFFF',400,48)+expected_text('two/note',56,540,'默认样式提供字体，局部设置可以关闭粗体。',24,'#FFFFFF',400,32)
    put('ppt/slides/slide2.xml',page(objects,f'<p:bg><p:bgRef idx="1001"><a:schemeClr val="accent1"/></p:bgRef></p:bg>'));expected.append(f'<Slide id="two" background="#087E6D">{xml}</Slide>')
    objects=text(2,56,48,'主题色变暗与颜色变换顺序')
    cases=[('accent1','<a:shade val="50000"/>','#045B4E'),('dk2','<a:lumMod val="50000"/><a:lumOff val="25000"/>','#606060'),('dk2','<a:lumOff val="25000"/><a:lumMod val="50000"/>','#404040')]
    xml=expected_text('three/title',56,48,'主题色变暗与颜色变换顺序')
    for i,(name,transform,resolved)in enumerate(cases):
        x=56+394*i;objects+=shape(3+i,x,220,340,240,color(name,transform));xml+=f'<Shape id="three/card-{i}" style="position:absolute;left:{x}px;top:220px;width:340px;height:240px;background:{resolved}"/>'
    objects+=text(6,56,540,'相同的变换，顺序不同，结果也应不同。',1800,bold=False,line=2400)
    xml+=expected_text('three/note',56,540,'相同的变换，顺序不同，结果也应不同。',24,bold=400,line=32)
    put('ppt/slides/slide3.xml',page(objects,f'<p:bg><p:bgPr>{color("lt1")}</p:bgPr></p:bg>'));expected.append(f'<Slide id="three" background="#FFFFFF">{xml}</Slide>')
    args.out.parent.mkdir(parents=True,exist_ok=True);args.expected.parent.mkdir(parents=True,exist_ok=True)
    with ZipFile(args.out,'w')as z:
        for info,data in parts.values():z.writestr(info,data)
    write_project(ET.fromstring('<Deck font-family="Noto Sans CJK SC">'+''.join(expected)+'</Deck>'), args.expected)

if __name__=='__main__':main()

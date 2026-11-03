"""Add empty OOXML text containers to owned demo slides for browser regression."""
import argparse
import xml.etree.ElementTree as ET
from pathlib import Path
from zipfile import ZipFile

P = 'http://schemas.openxmlformats.org/presentationml/2006/main'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
for prefix, namespace in [('p', P), ('a', A), ('r', R)]:
    ET.register_namespace(prefix, namespace)


def empty_object(ident, preset, width, height, y):
    geometry = f'<a:prstGeom prst="{preset}"><a:avLst/></a:prstGeom>' if preset else ''
    return ET.fromstring(f'''<p:sp xmlns:p="{P}" xmlns:a="{A}">
      <p:nvSpPr><p:cNvPr id="{ident}" name="Empty text regression {ident}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
      <p:spPr><a:xfrm><a:off x="476250" y="{y}"/><a:ext cx="{width}" cy="{height}"/></a:xfrm>
        {geometry}<a:noFill/><a:ln w="9525"><a:solidFill><a:srgbClr val="087E6D"/></a:solidFill></a:ln>
      </p:spPr>
      <p:txBody><a:bodyPr/><a:lstStyle/>
        <a:p><a:endParaRPr lang="en-US"/></a:p>
        <a:p><a:r><a:t xml:space="preserve"> </a:t></a:r></a:p>
      </p:txBody>
    </p:sp>''')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if args.base.resolve() == args.out.resolve():
        raise ValueError('fixture must not replace the input deck')
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with ZipFile(args.base) as source, ZipFile(args.out, 'w') as output:
        for info in source.infolist():
            data = source.read(info)
            if info.filename.startswith('ppt/slides/slide') and info.filename.endswith('.xml'):
                root = ET.fromstring(data)
                tree = root.find(f'./{{{P}}}cSld/{{{P}}}spTree')
                for offset, (preset, width, height) in enumerate([
                    ('line', 3657600, 0), ('rect', 5669280, 0),
                    ('ellipse', 1280160, 237744), (None, 7863840, 0),
                ]):
                    tree.append(empty_object(10000 + offset, preset, width, height, 6096000 + offset * 95250))
                data = ET.tostring(root, encoding='utf-8', xml_declaration=True)
            output.writestr(info, data)


if __name__ == '__main__':
    main()

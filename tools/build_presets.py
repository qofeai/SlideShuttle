"""Compile ECMA-376 presetShapeDefinitions.xml into the compact table the core embeds.

Usage: python3 tools/build_presets.py presetShapeDefinitions.xml > crates/slide-shuttle-core/data/presets.txt

Keeps adjust defaults, guides and paths; drops connection sites, handles and text rectangles.
Line format:
  shape <name>
  av <name> <formula>          adjust default
  gd <name> <formula>          guide
  path <w> <h> <fill> <stroke> w/h 0 = shape size; fill norm|none|lighten|lightenLess|darken|darkenLess
  M x y | L x y | A wR hR stAng swAng | Q x1 y1 x y | C x1 y1 x2 y2 x y | Z
"""
import sys
import xml.etree.ElementTree as ET

A = "{http://schemas.openxmlformats.org/drawingml/2006/main}"


def pts(cmd):
    return " ".join(f"{p.get('x')} {p.get('y')}" for p in cmd.findall(f"{A}pt"))


out = []
for shape in ET.parse(sys.argv[1]).getroot():
    out.append(f"shape {shape.tag}")
    for kind, tag in (("av", "avLst"), ("gd", "gdLst")):
        for gd in shape.findall(f"{A}{tag}/{A}gd"):
            out.append(f"{kind} {gd.get('name')} {gd.get('fmla')}")
    for path in shape.findall(f"{A}pathLst/{A}path"):
        out.append(
            f"path {path.get('w', '0')} {path.get('h', '0')} {path.get('fill', 'norm')} {path.get('stroke', 'true')}"
        )
        for cmd in path:
            tag = cmd.tag.removeprefix(A)
            if tag == "moveTo":
                out.append(f"M {pts(cmd)}")
            elif tag == "lnTo":
                out.append(f"L {pts(cmd)}")
            elif tag == "quadBezTo":
                out.append(f"Q {pts(cmd)}")
            elif tag == "cubicBezTo":
                out.append(f"C {pts(cmd)}")
            elif tag == "arcTo":
                out.append(f"A {cmd.get('wR')} {cmd.get('hR')} {cmd.get('stAng')} {cmd.get('swAng')}")
            elif tag == "close":
                out.append("Z")
            else:
                raise SystemExit(f"unknown path command {tag} in {shape.tag}")
print("\n".join(out))

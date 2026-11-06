"""Script-neutral visual audits: line measure, leading, contrast, accent hues, type scale, title anchoring.

Every rule is expressed in em, ratios or colour maths, so it applies to any writing system.
Warnings flag readability risks; hints flag contextual choices that need a human look.
"""
import colorsys
import copy
import re
import xml.etree.ElementTree as ET

MAX_MEASURE_EM = 32          # multi-line text wider than ~30em is hard to track back to the next line
MIN_BODY_LEADING = 1.25      # below 40px, multi-line text needs at least this line-height ratio
MIN_TITLE_LEADING = 1.05     # 40-72px multi-line headings
MAX_TEXT_SIZES = 5           # distinct reading sizes below 48px across the deck
TITLE_TOLERANCE_PX = 4
CONTENT_TYPES = {None, 'content', 'contents'}
INHERITED = ('color', 'font-size', 'font-weight')

NAMED = {'black': '#000000', 'white': '#FFFFFF'}
HEX_RE = re.compile(r'#[0-9A-Fa-f]{3,8}\b')
RGBA_RE = re.compile(r'rgba?\(\s*([0-9.]+)\s*,\s*([0-9.]+)\s*,\s*([0-9.]+)\s*(?:,\s*([0-9.]+%?)\s*)?\)')
VAR_RE = re.compile(r'var\(--([A-Za-z0-9_-]+)\)')
PARAM_RE = re.compile(r'\$\{([A-Za-z0-9_-]+)\}')


def parse_css(text):
    out = {}
    for part in (text or '').split(';'):
        if ':' in part:
            key, value = part.split(':', 1)
            out[key.strip().lower()] = value.strip()
    return out


def parse_color(value):
    """Return (r, g, b, a) in 0-1, or None for gradients, images and unknown values."""
    if not value:
        return None
    value = value.strip()
    if value.lower() in NAMED:
        value = NAMED[value.lower()]
    if value.lower() == 'transparent':
        return (0, 0, 0, 0)
    m = re.fullmatch(r'#([0-9A-Fa-f]{3,8})', value)
    if m:
        h = m.group(1)
        if len(h) in (3, 4):
            h = ''.join(c * 2 for c in h)
        if len(h) not in (6, 8):
            return None
        a = int(h[6:8], 16) / 255 if len(h) == 8 else 1.0
        return (int(h[0:2], 16) / 255, int(h[2:4], 16) / 255, int(h[4:6], 16) / 255, a)
    m = RGBA_RE.fullmatch(value)
    if m:
        alpha = m.group(4)
        a = 1.0 if alpha is None else (float(alpha[:-1]) / 100 if alpha.endswith('%') else float(alpha))
        return (float(m.group(1)) / 255, float(m.group(2)) / 255, float(m.group(3)) / 255, a)
    return None


def over(fg, bg):
    a = fg[3]
    return tuple(fg[i] * a + bg[i] * (1 - a) for i in range(3)) + (1.0,)


def luminance(c):
    def channel(v):
        return v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4
    return 0.2126 * channel(c[0]) + 0.7152 * channel(c[1]) + 0.0722 * channel(c[2])


def contrast(a, b):
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def px(value):
    m = re.fullmatch(r'([0-9.]+)px', (value or '').strip())
    return float(m.group(1)) if m else None


class Deck:
    def __init__(self, entry):
        self.entry = entry
        self.tokens, self.styles, self.components, self.slides = {}, {}, {}, []
        root = ET.parse(entry).getroot()
        self._collect(root)
        for node in root:
            if node.tag == 'Library' and 'src' in node.attrib:
                path = entry.parent / node.attrib['src']
                if path.is_file():
                    self._collect(ET.parse(path).getroot())
            elif node.tag == 'Slide':
                if 'src' in node.attrib:
                    path = entry.parent / node.attrib['src']
                    if path.is_file():
                        self.slides.append(ET.parse(path).getroot())
                else:
                    self.slides.append(node)

    def _collect(self, root):
        for node in root:
            if node.tag == 'Token' and 'name' in node.attrib:
                self.tokens[node.attrib['name']] = node.attrib.get('value', '')
            elif node.tag == 'Style' and 'name' in node.attrib:
                self.styles[node.attrib['name']] = (node.text or '').strip()
            elif node.tag == 'Component' and 'name' in node.attrib:
                self.components[node.attrib['name']] = node

    def resolve(self, value):
        for _ in range(16):
            new = VAR_RE.sub(lambda m: self.tokens.get(m.group(1), m.group(0)), value)
            if new == value:
                break
            value = new
        return value

    def own_style(self, elem):
        css = {}
        for cls in elem.attrib.get('class', '').split():
            css.update(parse_css(self.styles.get(cls, '')))
        css.update(parse_css(elem.attrib.get('style', '')))
        return {k: self.resolve(v) for k, v in css.items()}

    def expand(self, use):
        """Instantiate a Use as a detached subtree with substituted parameters and prefixed IDs."""
        template = self.components.get(use.attrib.get('component', ''))
        if template is None or len(template) == 0:
            return None
        params = {k: v for k, v in use.attrib.items() if k not in ('id', 'component')}
        root = copy.deepcopy(template[0])
        prefix = use.attrib.get('id', '')
        for node in root.iter():
            for key, value in list(node.attrib.items()):
                node.attrib[key] = PARAM_RE.sub(lambda m: params.get(m.group(1), m.group(0)), value)
            if node.text:
                node.text = PARAM_RE.sub(lambda m: params.get(m.group(1), m.group(0)), node.text)
            if 'id' in node.attrib:
                node.attrib['id'] = f'{prefix}/{node.attrib["id"]}'
        return root


def walk(deck, elem, inherited, out, paints):
    """Collect every Text with its inherited style, and every solid fill in paint order."""
    if elem.tag == 'Use':
        expanded = deck.expand(elem)
        if expanded is not None:
            walk(deck, expanded, inherited, out, paints)
        return
    if elem.tag in ('Notes', 'Animate'):
        return
    css = deck.own_style(elem)
    computed = dict(inherited)
    for key in INHERITED:
        if key in css:
            computed[key] = css[key]
    size = px(computed.get('font-size'))
    if 'line-height' in css:
        lh = css['line-height']
        lh_px = px(lh)
        if lh_px is not None and size:
            computed['leading'] = lh_px / size
        else:
            try:
                computed['leading'] = float(lh)
            except ValueError:
                pass
    paint = css.get('background') or css.get('background-color')
    if paint and 'id' in elem.attrib:
        # None marks a gradient or image fill: text over it cannot be judged here
        paints.append((elem.attrib['id'], parse_color(paint), len(out)))
    if elem.tag == 'Text':
        out.append((elem, computed, css))
        return
    for child in elem:
        walk(deck, child, computed, out, paints)


def backdrop(index, center, paints, nodes, slide_bg):
    """Colour behind a Text: the last fill painted before it that covers its centre."""
    color = slide_bg
    for nid, fill, order in paints:
        if order > index:
            break
        rect = (nodes.get(nid) or {}).get('rect')
        if not rect or not (rect['x'] <= center[0] <= rect['x'] + rect['width'] and rect['y'] <= center[1] <= rect['y'] + rect['height']):
            continue
        if fill is None:
            color = None
        elif fill[3] > 0 and color is not None:
            color = over(fill, color)
    return color


def colors_in(deck):
    """Every concrete colour the deck paints with."""
    found = []
    def scan(value):
        value = deck.resolve(value or '')
        for hit in HEX_RE.findall(value) + [m.group(0) for m in RGBA_RE.finditer(value)]:
            c = parse_color(hit)
            if c:
                found.append(c)
    for css in deck.styles.values():
        scan(css)
    for slide in deck.slides:
        scan(slide.attrib.get('background'))
        for node in slide.iter():
            scan(node.attrib.get('style'))
            for key in ('color', 'colors'):
                scan(node.attrib.get(key))
            if node.tag == 'Use':
                for key, value in node.attrib.items():
                    if key not in ('id', 'component'):
                        scan(value)
    for comp in deck.components.values():
        for node in comp.iter():
            scan(node.attrib.get('style'))
    return found


def accent_hues(colors):
    """Cluster saturated colours by hue; neutrals (greys, near-black, tints) are ignored."""
    hues = []
    for r, g, b, a in colors:
        if a < 0.5:
            continue
        h, s, v = colorsys.rgb_to_hsv(r, g, b)
        if s >= 0.4 and v >= 0.35:
            hues.append(h * 360)
    clusters = []
    for hue in sorted(hues):
        if clusters and min(abs(hue - clusters[-1][-1]), 360 - abs(hue - clusters[-1][-1])) <= 25:
            clusters[-1].append(hue)
        else:
            clusters.append([hue])
    if len(clusters) > 1 and 360 - clusters[-1][-1] + clusters[0][0] <= 25:
        clusters[0] = clusters.pop() + clusters[0]
    return [round(sum(c) / len(c)) for c in clusters]


def audit_visual(entry, check):
    warnings, hints = [], []
    try:
        deck = Deck(entry)
    except (ET.ParseError, OSError):
        return warnings, hints
    pages = {p['id']: {n['id']: n for n in p.get('nodes', [])} for p in (check or {}).get('pages', [])}
    sizes = set()
    title_anchor = None

    def warn(code, page, node, message):
        destination = hints if code in {'missing_title', 'title_position_shift', 'type_scale_sprawl'} else warnings
        destination.append({'code': code, 'page': page, 'nodeId': node, 'message': message})

    for slide in deck.slides:
        sid = slide.attrib.get('id')
        nodes = pages.get(sid, {})
        bg = parse_color(slide.attrib.get('background'))
        texts, paints = [], []
        for child in slide:
            walk(deck, child, {}, texts, paints)
        slide_bg = bg if bg and bg[3] == 1 else None

        title = next((t for t in texts if t[0].attrib.get('id') == 'title'), None)
        if title is None:
            warn('missing_title', sid, sid, f'Slide "{sid}" has no <Text id="title">. Add one if a visible title helps; image-only and section slides may omit it. outline.md uses this id when present.')
        elif slide.attrib.get('type') in CONTENT_TYPES and 'title' in nodes:
            rect = nodes['title']['rect']
            if title_anchor is None:
                title_anchor = (sid, rect['x'], rect['y'])
            elif abs(rect['x'] - title_anchor[1]) > TITLE_TOLERANCE_PX or abs(rect['y'] - title_anchor[2]) > TITLE_TOLERANCE_PX:
                warn('title_position_shift', sid, 'title', f'Title starts at ({rect["x"]:.0f}, {rect["y"]:.0f}) but "{title_anchor[0]}" starts at ({title_anchor[1]:.0f}, {title_anchor[2]:.0f}). Check whether this is an intentional layout change.')
        if slide.find('Notes') is None:
            hints.append({'code': 'missing_notes', 'page': sid, 'nodeId': sid, 'message': f'Slide "{sid}" has no <Notes>. What was cut from the canvas belongs in the speaker notes.'})

        for index, (elem, style, own) in enumerate(texts):
            nid = elem.attrib.get('id', 'Text')
            size = px(style.get('font-size'))
            if size:
                sizes.add(size)
            for span in elem.iter('Span'):
                span_size = px(parse_css(span.attrib.get('style', '')).get('font-size'))
                if span_size:
                    sizes.add(span_size)
            if 'height' in own:
                hints.append({'code': 'text_fixed_height', 'page': sid, 'nodeId': nid, 'message': f'Text "{nid}" has a fixed height. Let text size itself and give the remaining space to a flex-grow body; fixed heights cause empty bands and overflow.'})
            node = nodes.get(nid)
            lines = (node or {}).get('lineCount') or 0
            breaks = sum(1 for _ in elem.iter('br'))
            soft_wrapped = lines > breaks + 1
            if node and size and soft_wrapped and node['rect']['width'] / size > MAX_MEASURE_EM:
                warn('line_too_long', sid, nid, f'Text "{nid}" wraps across a {node["rect"]["width"] / size:.0f}em measure (limit {MAX_MEASURE_EM}em). Narrow the column or shorten the copy so the eye can find the next line.')
            leading = style.get('leading')
            if size and leading and lines >= 2:
                floor = MIN_BODY_LEADING if size < 40 else MIN_TITLE_LEADING if size < 72 else None
                if floor and leading < floor:
                    warn('tight_leading', sid, nid, f'Text "{nid}" runs {lines} lines at {leading:.2f}× line-height (floor {floor}× at {size:g}px). Scripts with tall glyphs or stacked marks need even more.')
            fg = parse_color(style.get('color', '#000000'))
            background = None
            if node:
                r = node['rect']
                # Fills painted before this Text (including its own) carry an order <= index
                background = backdrop(index, (r['x'] + r['width'] / 2, r['y'] + r['height'] / 2), paints, nodes, slide_bg)
            if fg and background and fg[3] > 0 and size:
                ratio = contrast(over(fg, background), background)
                bold = style.get('font-weight') == '700'
                large = size >= 24 or (bold and size >= 18.66)
                floor = 3.0 if large else 4.5
                if ratio < floor:
                    warn('low_contrast', sid, nid, f'Text "{nid}" has {ratio:.1f}:1 contrast against its background (needs {floor}:1 at {size:g}px). Demote secondary text with size and position, not by fading it.')

    reading = sorted(s for s in sizes if s < 48)
    if len(reading) > MAX_TEXT_SIZES:
        warn('type_scale_sprawl', 'deck', 'deck', f'The deck uses {len(reading)} reading sizes below 48px ({", ".join(f"{s:g}" for s in reading)}). Check that the sizes have distinct roles and shared styles; additional levels may be appropriate.')
    hues = accent_hues(colors_in(deck))
    if len(hues) > 2:
        hints.append({'code': 'palette_review', 'page': 'deck', 'nodeId': 'deck',
                      'message': f'The source palette defines {len(hues)} saturated hues. Check that category, status and brand colours have consistent roles; multiple hues are allowed and are not a failure.'})
    return warnings, hints

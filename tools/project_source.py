"""Materialize historical inline XML fixtures as a multi-file authoring project."""
import copy
from pathlib import Path
import xml.etree.ElementTree as ET


def write_project(root, entry):
    entry = Path(entry)
    root = copy.deepcopy(root)
    folder = entry.parent / (entry.stem + '-pages')
    folder.mkdir(parents=True, exist_ok=True)
    for index, node in enumerate(list(root)):
        if node.tag == 'Slide' and 'src' not in node.attrib:
            name = f'page-{index + 1}.xml'
            (folder / name).write_text(ET.tostring(node, encoding='unicode'), encoding='utf-8')
            root.remove(node)
            root.insert(index, ET.Element('Slide', {'src': folder.name + '/' + name}))
    entry.write_text(ET.tostring(root, encoding='unicode'), encoding='utf-8')
    return entry

"""Independent structural checks, not a replacement for real Office rendering.

pip install python-pptx==1.0.2 openpyxl==3.1.5 pymupdf==2.0
python tools/verify_outputs.py OUTPUT.pptx PREVIEW_DIR
"""
import io
import json
import sys
import zipfile
from pathlib import Path

import pymupdf
from openpyxl import load_workbook
from pptx import Presentation


def walk(shapes):
    for shape in shapes:
        yield shape
        if hasattr(shape, "shapes"):
            yield from walk(shape.shapes)


def verify(pptx, preview):
    deck = Presentation(pptx)
    counts = {key: 0 for key in ("text", "group", "chart", "table")}
    for slide in deck.slides:
        for shape in walk(slide.shapes):
            for key, value in (("text", shape.has_text_frame), ("group", hasattr(shape, "shapes")), ("chart", shape.has_chart), ("table", shape.has_table)):
                counts[key] += bool(value)
            if shape.has_chart:
                assert list(shape.chart.series[0].values) == [28, 45, 63, 82]
            if shape.has_table:
                assert shape.table.cell(1, 1).text == "原生文本框"
    assert len(deck.slides) == 3
    assert counts["group"] == counts["chart"] == counts["table"] == 1
    with zipfile.ZipFile(pptx) as package:
        assert not any(name.endswith("base.json") for name in package.namelist())
        for name in package.namelist():
            if name.endswith(".xlsx"):
                workbook = load_workbook(io.BytesIO(package.read(name)))
                assert workbook.active["B5"].value == 82
    pdf = pymupdf.open(Path(preview) / "deck.pdf")
    assert len(pdf) == 3
    for index, needle in enumerate(("让想法", "中文原生排版", "原生文本框")):
        assert needle in pdf[index].get_text()
        assert pdf[index].search_for(needle)
    print(json.dumps({"pptxPages": len(deck.slides), "nativeObjects": counts, "pdfPages": len(pdf), "pdfCjkSearch": True, "embeddedWorkbookValues": True, "officeRenderingValidated": False}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    verify(*sys.argv[1:])

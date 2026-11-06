import os
import json
import stat
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
import zipfile
from pathlib import Path

sys.path.insert(0,str(Path(__file__).resolve().parent))
sys.path.insert(0,str(Path(__file__).resolve().parent.parent / "skills/slideshuttle/scripts"))
import build as skill_build
from evaluate_pilot import extract_deck,normal,potential_overlaps,quoted_requirements
from render_and_score import structure


class HarnessTests(unittest.TestCase):
    def test_failed_build_with_a_delivery_is_never_deleted_on_retry(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory)
            (out / 'deliverables').mkdir()
            artifact = out / 'deliverables/presentation.qppt'
            artifact.write_bytes(b'preserve the completed package')
            (out / 'build.json').write_text(json.dumps({'status': 'failed'}))
            with self.assertRaises(ValueError):
                skill_build.prepare_out_dir(out)
            self.assertEqual(artifact.read_bytes(), b'preserve the completed package')

    def test_incomplete_generation_is_not_a_success(self):
        with self.assertRaises(ValueError):extract_deck("<Deck><Slide/>")
        self.assertEqual(extract_deck('```xml\n<Deck><Slide/></Deck>\n```'),"<Deck><Slide/></Deck>")

    def test_content_check_normalizes_typography_but_does_not_invent_paraphrase_credit(self):
        self.assertEqual(normal("Airbnb’s 2021\nrelease."),normal("AIRBNB'S 2021 release"))
        self.assertNotEqual(normal("Release 100 upgrades"),normal("Many improvements released"))
        self.assertEqual(quoted_requirements('Title "References" and repeat "References", then “Branding”'),["References","Branding"])

    def test_background_boxes_are_not_text_overlap_failures(self):
        a={"id":"text","lineCount":1,"rect":{"x":0,"y":0,"width":100,"height":30}}
        bg={"id":"background","lineCount":None,"rect":{"x":0,"y":0,"width":100,"height":30}}
        self.assertEqual(potential_overlaps([a,bg]),[])
        b={**a,"id":"other"};self.assertEqual(len(potential_overlaps([a,b])),1)

    def test_blank_master_and_chart_do_not_get_l4_credit(self):
        with tempfile.TemporaryDirectory()as directory:
            path=Path(directory)/"fixture.pptx"
            with zipfile.ZipFile(path,"w")as z:
                z.writestr("ppt/slideMasters/slideMaster1.xml",'<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr/><p:grpSpPr/></p:spTree></p:cSld></p:sldMaster>')
                z.writestr("ppt/charts/chart1.xml","<chart/>")
                z.writestr("ppt/embeddings/chart1.xlsx",b"fixture")
            result=structure(path)
            self.assertEqual(result["masters"],1)
            self.assertEqual(result["nativeCharts"],1)
            self.assertEqual(result["peiUpperBound"],2)
            self.assertFalse(result["peiL3MasterInheritanceVerified"])

    def test_build_discovers_cli_and_fonts_without_repo(self):
        with tempfile.TemporaryDirectory() as directory:
            prefix = Path(directory) / "prefix"
            (prefix / "bin").mkdir(parents=True)
            (prefix / "share/fonts").mkdir(parents=True)
            cli = prefix / "bin/slide-shuttle"
            cli.write_bytes(b"binary")
            cli.chmod(cli.stat().st_mode | stat.S_IXUSR)
            for weight in ["Regular", "Bold"]:
                (prefix / f"share/fonts/NotoSansCJK-{weight}.ttc").write_bytes(b"font")
            args = types.SimpleNamespace(repo=None, cli=None, font=[], doctor=True, source=None)
            fake_proc = types.SimpleNamespace(stdout="slide-shuttle 0.3.0\n", returncode=0)
            with patch.object(skill_build, "repository", return_value=None), patch.object(
                skill_build.shutil, "which", return_value=str(cli)
            ), patch.object(skill_build.subprocess, "run", return_value=fake_proc):
                info = skill_build.runtime(args)
            self.assertEqual(info["cli"], str(cli.resolve()))
            self.assertEqual(
                info["fonts"],
                [
                    str((prefix / "share/fonts/NotoSansCJK-Regular.ttc").resolve()),
                    str((prefix / "share/fonts/NotoSansCJK-Bold.ttc").resolve()),
                ],
            )


if __name__=="__main__":unittest.main()

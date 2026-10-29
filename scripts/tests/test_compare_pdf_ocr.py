"""OCR comparison geometry checks distinguish validity from agreement."""
import copy
from pathlib import Path
import sys
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from compare_pdf_ocr import agreement, geometry


def document(box):
    return {'layout': {'pages': [{'width': 100, 'height': 100, 'spans': [
        {'id': 0, 'ocr': True, 'words': [{'text': 'Hello', 'bbox': box}]}]}]}, 'stats': {'tables': 0}}


class OcrGeometry(unittest.TestCase):
    def test_agreement_compares_positions_for_matching_text(self):
        a = document([1, 2, 11, 12])
        self.assertEqual(agreement(a, copy.deepcopy(a))['mean_matched_word_bbox_iou'], 1)
        self.assertEqual(agreement(a, document([20, 20, 30, 30]))['mean_matched_word_bbox_iou'], 0)
        self.assertEqual(geometry(a)['words'], 1)

    def test_invalid_geometry_cannot_pass_via_text_quality(self):
        for box in [[0, 0, float('nan'), 1], [-1, 0, 10, 10], [1, 1, 1, 2], [0, 0, 101, 10]]:
            with self.subTest(box=box), self.assertRaises(AssertionError):
                geometry(document(box))

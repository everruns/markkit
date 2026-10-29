"""Pinned OCR model cache keeps verified assets and rejects corrupt downloads."""
import hashlib
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import prepare_ocr_models as models


class PrepareOcrModels(unittest.TestCase):
    def test_pinned_download_and_offline_cache(self):
        wanted = b'model weights'
        pin = dict(name='model.onnx', size=len(wanted), sha256=hashlib.sha256(wanted).hexdigest(), url='https://example.com/pinned')
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with patch.object(models.urllib.request, 'urlopen', return_value=io.BytesIO(wanted)):
                path = models.prepare(pin, output)
            self.assertEqual(path.read_bytes(), wanted)
            with patch.object(models.urllib.request, 'urlopen', side_effect=AssertionError('network forbidden')):
                self.assertEqual(models.prepare(pin, output), path)

    def test_corrupt_and_oversized_models_do_not_replace_existing_asset(self):
        wanted = b'good'
        pin = dict(name='model.onnx', size=4, sha256=hashlib.sha256(wanted).hexdigest(), url='https://example.com/pinned')
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            target = output / pin['name']
            target.write_bytes(b'previous')
            for data in [b'evil', b'oversized', b'x']:
                with self.subTest(data=data), patch.object(models.urllib.request, 'urlopen', return_value=io.BytesIO(data)):
                    with self.assertRaises(ValueError):
                        models.prepare(pin, output)
                self.assertEqual(target.read_bytes(), b'previous')
                self.assertEqual(list(output.iterdir()), [target])

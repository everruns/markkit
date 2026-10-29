"""Structured byte extraction, including PDF tables and source geometry."""
import unittest
import markkit


def pdf(content):
    return ("%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n"
            "2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n"
            "3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >> endobj\n"
            "4 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n"
            f"5 0 obj << /Length {len(content)} >> stream\n{content}\nendstream endobj\n"
            "trailer << /Root 1 0 R >>\n%%EOF").encode()


class Documents(unittest.TestCase):
    def test_pdf_layout_and_limits(self):
        data = pdf("BT /F1 10 Tf 10 100 Td (Hi) Tj ET")
        plain = markkit.extract_document_sync(data, file_name="a.pdf")
        self.assertNotIn("layout", plain)
        e = markkit.extract_document_sync(data, file_name="a.pdf", pdf_layout=True)
        self.assertEqual(e["markdown"], plain["markdown"])
        self.assertEqual(e["layout"]["pages"][0]["blocks"][0]["kind"], "paragraph")
        self.assertEqual(e["layout"]["pages"][0]["spans"][0]["baseline"], [[10, 300], [20, 300]])
        with self.assertRaisesRegex(RuntimeError, "max_nodes"):
            markkit.extract_document_sync(data, file_name="a.pdf", pdf_layout=True, limits=markkit.Limits(max_nodes=1))

    def test_ocr_backend_validation_and_lazy_models(self):
        data = pdf("BT /F1 10 Tf 10 100 Td (Hi) Tj ET")
        with self.assertRaisesRegex(ValueError, "tesseract or ocrs"):
            markkit.extract_document_sync(data, pdf_ocr_backend="unknown")
        result = markkit.extract_document_sync(data, pdf_ocr=True, pdf_ocr_backend="ocrs", pdf_ocr_detection_model="unused.onnx", pdf_ocr_recognition_model="unused.onnx")
        self.assertEqual(result["markdown"], "Hi\n")

    def test_borderless_table(self):
        content = "\n".join(f"BT /F1 10 Tf 10 {y} Td ({a}) Tj ET BT /F1 10 Tf 150 {y} Td ({b}) Tj ET" for y, a, b in [(300, "Item", "Qty"), (280, "Apples", "3"), (260, "Pears", "2")])
        e = markkit.extract_document_sync(pdf(content), file_name="a.pdf", pdf_layout=True)
        self.assertEqual(e["stats"]["tables"], 1)
        self.assertIn("| Apples | 3 |", e["markdown"])
        self.assertEqual(e["layout"]["pages"][0]["blocks"][0]["kind"], "table")


if __name__ == "__main__":
    unittest.main()

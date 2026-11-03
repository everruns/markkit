"""markkit.pyi (shipped in the wheel) must match the native module."""
import ast
import inspect
import pathlib
import unittest

import markkit

STUB = pathlib.Path(__file__).resolve().parents[3] / "markkit.pyi"


def stub_tree():
    return ast.parse(STUB.read_text())


def stub_params(fn):
    a = fn.args
    return [p.arg for p in a.posonlyargs + a.args + a.kwonlyargs if p.arg != "self"]


class Stubs(unittest.TestCase):
    def test_functions_match_runtime_signatures(self):
        stub_fns = {n.name: n for n in stub_tree().body if isinstance(n, ast.FunctionDef)}
        runtime = {
            name for name in dir(markkit)
            if not name.startswith("_") and inspect.isbuiltin(getattr(markkit, name))
        }
        self.assertEqual(set(stub_fns), runtime)
        for name, fn in stub_fns.items():
            sig = inspect.signature(getattr(markkit, name))
            self.assertEqual(stub_params(fn), list(sig.parameters), name)

    def test_limits_fields_match_runtime(self):
        cls = next(n for n in stub_tree().body if isinstance(n, ast.ClassDef) and n.name == "Limits")
        fields = [n.target.id for n in cls.body if isinstance(n, ast.AnnAssign)]
        init = next(n for n in cls.body if isinstance(n, ast.FunctionDef) and n.name == "__init__")
        self.assertEqual(stub_params(init), fields)
        self.assertEqual(fields, list(inspect.signature(markkit.Limits).parameters))
        limits = markkit.Limits()
        for f in fields:
            self.assertTrue(hasattr(limits, f), f)

    def test_all_lists_public_names(self):
        tree = stub_tree()
        declared = next(
            ast.literal_eval(n.value) for n in tree.body
            if isinstance(n, ast.Assign) and n.targets[0].id == "__all__"
        )
        defined = {n.name for n in tree.body if isinstance(n, (ast.FunctionDef, ast.ClassDef))}
        self.assertEqual(set(declared), defined)


if __name__ == "__main__":
    unittest.main()

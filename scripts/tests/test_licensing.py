"""Markkit is MIT licensed, and every published artifact must say so and ship
the license text plus NOTICE.

Decisions:
- One source of truth: root LICENSE and NOTICE. crates.io crates carry
  symlinks (cargo packages the target's contents). npm skips symlinks, so the
  Node package keeps byte-identical copies; the WASM build copies them into pkg/.
- Python takes `license`/`license-files` statically from pyproject.toml
  (PEP 639) so wheels and sdists carry License-Expression and both files.
"""

import json
import pathlib
import re
import tomllib
import unittest

REPO = pathlib.Path(__file__).resolve().parents[2]
CRATES_IO = ["markkit", "markkit-cli"]
NPM = ["markkit-js", "markkit-wasm"]
LICENSE_FILES = ["LICENSE", "NOTICE"]


def load_toml(path):
    with open(path, "rb") as f:
        return tomllib.load(f)


class LicensingTest(unittest.TestCase):
    def test_root_license_is_mit(self):
        text = (REPO / "LICENSE").read_text()
        self.assertTrue(text.startswith("MIT License\n"))
        self.assertRegex(text, r"Copyright \(c\) [0-9-]+ Everruns")
        self.assertIn("Licensed under the MIT License.", (REPO / "NOTICE").read_text())

    def test_workspace_and_every_crate_declare_mit(self):
        ws = load_toml(REPO / "Cargo.toml")
        self.assertEqual(ws["workspace"]["package"]["license"], "MIT")
        for member in ws["workspace"]["members"]:
            pkg = load_toml(REPO / member / "Cargo.toml")["package"]
            with self.subTest(crate=member):
                self.assertEqual(pkg.get("license"), {"workspace": True})
                self.assertNotIn("license-file", pkg)

    def test_crates_io_crates_ship_license_and_notice(self):
        for crate in CRATES_IO:
            for name in LICENSE_FILES:
                path = REPO / "crates" / crate / name
                with self.subTest(crate=crate, file=name):
                    self.assertTrue(path.is_symlink(), f"{path} must link to root {name}")
                    self.assertEqual(path.resolve(), (REPO / name).resolve())

    def test_npm_packages_declare_mit_and_ship_files(self):
        for crate in NPM:
            pkg = json.loads((REPO / "crates" / crate / "package.json").read_text())
            with self.subTest(package=pkg["name"]):
                self.assertEqual(pkg["license"], "MIT")
                for name in LICENSE_FILES:
                    self.assertIn(name, pkg["files"])

    def test_node_package_copies_match_root(self):
        for name in LICENSE_FILES:
            copy = REPO / "crates" / "markkit-js" / name
            with self.subTest(file=name):
                self.assertFalse(copy.is_symlink(), "npm pack skips symlinks")
                self.assertEqual(copy.read_bytes(), (REPO / name).read_bytes())

    def test_wasm_build_copies_license_files(self):
        script = (REPO / "crates" / "markkit-wasm" / "scripts" / "build.sh").read_text()
        self.assertRegex(script, r'cp "\$CRATE_DIR/\.\./\.\./LICENSE" "\$CRATE_DIR/\.\./\.\./NOTICE"')

    def test_python_package_declares_mit_and_ships_files(self):
        project = load_toml(REPO / "pyproject.toml")["project"]
        self.assertEqual(project["license"], "MIT")
        self.assertEqual(project["license-files"], LICENSE_FILES)
        self.assertNotIn("license", project.get("dynamic", []))
        # PEP 639: license classifiers conflict with a license expression.
        for classifier in project.get("classifiers", []):
            self.assertFalse(classifier.startswith("License ::"), classifier)

    def test_notice_covers_hayro_embedded_assets(self):
        hayro = load_toml(REPO / "Cargo.toml")["workspace"]["dependencies"]["hayro"]
        notice = (REPO / "NOTICE").read_text()
        features = hayro.get("features", [])
        if "embed-fonts" in features:
            self.assertIn("Copyright 2014 PDFium Authors", notice)
            self.assertIn("Foxit Software Inc.", notice)
        if "embed-cmaps" in features:
            self.assertIn("Copyright 1990-2023 Adobe", notice)

    def test_security_policy_lists_current_minor(self):
        version = load_toml(REPO / "Cargo.toml")["workspace"]["package"]["version"]
        minor = ".".join(version.split(".")[:2])
        policy = (REPO / "SECURITY.md").read_text()
        self.assertRegex(policy, rf"\|\s*{re.escape(minor)}\.x\s*\|\s*Yes")


if __name__ == "__main__":
    unittest.main()

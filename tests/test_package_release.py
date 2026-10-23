import shutil
import unittest
from unittest import mock

from tools import package_release


@unittest.skipUnless(shutil.which("cargo"), "needs cargo")
class ThirdPartyNotices(unittest.TestCase):
    def test_lists_crates_and_python_and_shares_texts(self):
        # The importer's packages are only installed for packaging (requirements-release.txt).
        with mock.patch.object(package_release, "BUNDLED_PYTHON", []):
            text = package_release.third_party_notices()
        self.assertIn("\nbevy 0.", text)
        self.assertIn("\nPython 3.", text)
        self.assertNotIn("\nopenprocyon ", text)
        # Crates sharing a licence text get it once (Apache-2.0 copies that differ in more than whitespace stay separate).
        apache = text.count(
            "TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION"
        )
        self.assertLess(apache, text.count("Apache-2.0\n") // 2)


if __name__ == "__main__":
    unittest.main()

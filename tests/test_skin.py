"""Skinned animals (tools/skin.py): curve and rotation helpers, and the imported dragon's skin lining up with its bones."""

import json
import math
import struct
import unittest

import numpy as np

from tests.imported import ASSETS
from tools.skin import _hermite, _quat, _rot


class Helpers(unittest.TestCase):
    def test_two_key_kochanek_bartels_is_linear_and_hermite_uses_tangents(self):
        kb = [
            [0, 0, 2, 0, 0, 0, 9, 9],
            [1, 1, 2, 0, 0, 0, 9, 9],
        ]  # stored tangents ignored
        self.assertAlmostEqual(_hermite(kb, 0.3), 0.3)
        self.assertAlmostEqual(
            _hermite([[0, 0, 0, 0, 0, 0, 0, 0], [2, 4, 0, 0, 0, 0, 0, 0]], 0.5), 1
        )
        keys = [[0, 0, 1, 0, 0, 0, 0, 1], [1, 1, 1, 0, 0, 0, 1, 0]]
        self.assertAlmostEqual(_hermite(keys, 0), 0)
        self.assertAlmostEqual(_hermite(keys, 1), 1)
        self.assertAlmostEqual(
            _hermite(keys, 0.5), 0.5
        )  # slope 1 at both ends: a straight line
        self.assertEqual(_hermite(keys[:1], 0.7), 0)

    def test_yaw_turns_about_up_then_pitch_about_starboard(self):
        np.testing.assert_allclose(
            _rot([math.pi / 2, 0, 0]) @ [0, 1, 0], [-1, 0, 0], atol=1e-9
        )
        np.testing.assert_allclose(
            _rot([0, math.pi / 2, 0]) @ [0, 1, 0], [0, 0, 1], atol=1e-9
        )
        q = _quat(_rot([0.3, 0, 0]))
        self.assertAlmostEqual(q[3], math.cos(0.15))


def _glb(path):
    b = path.read_bytes()
    n = struct.unpack("<I", b[12:16])[0]
    return json.loads(b[20 : 20 + n]), b[20 + n + 8 :]


@unittest.skipUnless(
    (ASSETS / "models/scenery/SpaceDragon.glb").is_file(), "needs imported game assets"
)
class Dragon(unittest.TestCase):
    def test_bones_whose_rest_is_the_bind_pose_cancel_their_inverse_bind(self):
        j, bin_ = _glb(ASSETS / "models/scenery/SpaceDragon.glb")
        if not j.get("skins"):
            self.skipTest("imported before animal skins (re-import)")
        self.assertEqual(j["animations"][0]["name"], "ANIMATION00")
        parent = {c: i for i, n in enumerate(j["nodes"]) for c in n.get("children", [])}

        def local(i):
            n, m = j["nodes"][i], np.eye(4)
            x, y, z, w = n.get("rotation", [0, 0, 0, 1])
            m[:3, :3] = [
                [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
            ]
            m[:3, 3] = n.get("translation", [0, 0, 0])
            return m

        def world(i):
            return local(i) if i not in parent else world(parent[i]) @ local(i)

        skin = j["skins"][0]
        a = j["accessors"][skin["inverseBindMatrices"]]
        off = j["bufferViews"][a["bufferView"]]["byteOffset"]
        ibm = struct.unpack_from(f"<{16 * a['count']}f", bin_, off)
        names = [j["nodes"][k]["name"] for k in skin["joints"]]
        for name in ("spine1", "left_thigh", "right_foot", "left_bicep"):
            k = names.index(name)
            m = (
                world(skin["joints"][k])
                @ np.array(ibm[16 * k : 16 * k + 16]).reshape(4, 4).T
            )
            np.testing.assert_allclose(m, np.eye(4), atol=0.01, err_msg=name)


if __name__ == "__main__":
    unittest.main()

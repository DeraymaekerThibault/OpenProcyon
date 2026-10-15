import unittest

from tools.hdt import parse

SAMPLE = """Specific Resolution ChunkArray - Element
{
\t00000024 A
\t{
\t\tTextureName - NoCursor   ( Navy    ) String '640hud_01'
\t\tTextureArea - NoCursor   ( Navy    ) Rect( 31, 181, 87, 256 )\t# left,top,right,bottom
\t)
\t00000006 B
\t{
\t\tTopLeft X ( Navy    ) Int -5
\t}
}
"""


class HdtTest(unittest.TestCase):
    def test_parse_tolerates_unbalanced_braces(self):
        j = parse(SAMPLE)
        self.assertEqual(j["A"]["Navy"]["TextureArea - NoCursor"], [31, 181, 87, 256])
        self.assertEqual(j["A"]["Navy"]["TextureName - NoCursor"], "640hud_01")
        self.assertEqual(j["B"]["Navy"]["TopLeft X"], -5)


if __name__ == "__main__":
    unittest.main()

import unittest

from tools.autoplay import summarise

LOG = """INFO openprocyon: profile real=5.0 mission=4.0 objective=None ended=false frame_ms=16.20 entities=900 ships=5 rocks=0 effects=3 particles=40 mesh_assets=10 audio=1
INFO openprocyon: profile real=10.0 mission=40.0 objective=Some(1) ended=false frame_ms=31.50 entities=1200 ships=5 rocks=0 effects=9 particles=25 mesh_assets=10 audio=2
INFO openprocyon::smoke: autoplay end outcome="time limit" time=3601 never_fired=["Objective 5 (Dock)", "Say \\"hi\\"", "Fail"]
INFO openprocyon::smoke: autoplay unfired story_win=1 story=1 win=0 loss=1 presentation=1
INFO openprocyon::smoke: autoplay blocked rule="Objective 5 (Dock)" class=story false=["Enter Volume(Volume Name=Dock) false 900s [player outside, 40 from edge]"] player=[1, 0, 2] bot: goal=None
"""


class Summarise(unittest.TestCase):
    def test_outcome_peaks_and_never_fired_split_by_kind(self):
        self.assertEqual(
            summarise(LOG, 0), ("time limit", "3601", [31.5, 1200, 40], 1, 2)
        )

    def test_log_without_the_split_line_counts_all_rules_as_other(self):
        old = "\n".join(x for x in LOG.splitlines() if "unfired" not in x)
        self.assertEqual(summarise(old, 0)[3:], ("?", 3))

    def test_panic_and_crash_without_end_line(self):
        self.assertEqual(
            summarise("thread 'main' panicked at src/x.rs:1", 101)[0], "PANIC"
        )
        self.assertEqual(
            summarise("", "timeout"), ("exit timeout", "?", [0.0, 0, 0], "?", "?")
        )


if __name__ == "__main__":
    unittest.main()

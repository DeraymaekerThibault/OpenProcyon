import json
import unittest

from tools.battle_check import evaluate, outcome


def report(
    player_bursts=5, side1_bursts=5, hits=3, first=20.0, sailed=500.0, contact=0.0
):
    return {
        "time": 120.0,
        "closest": 30.0,
        "sides": {
            "player": {
                "ships": 2,
                "alive": 2,
                "bursts": player_bursts,
                "hits": hits,
                "first_burst": 10.0,
            },
            "side1": {
                "ships": 1,
                "alive": 1,
                "bursts": side1_bursts,
                "hits": hits,
                "first_burst": first,
            },
        },
        "ships": [
            {
                "class": "Ship_Navy_Cutter",
                "side": "player",
                "sailed": 0.0,
                "worst_contact": 0.0,
            },
            {
                "class": "Ship_Pirate_Barque",
                "side": "side1",
                "sailed": sailed,
                "worst_contact": contact,
            },
        ],
    }


class Evaluate(unittest.TestCase):
    def test_a_healthy_fight_passes(self):
        self.assertEqual(evaluate(report(), {"fight", "patrol"}), [])

    def test_a_silent_side_fails_the_fight(self):
        # The map-script regression: neither side's computer-steered ships fired.
        self.assertEqual(
            evaluate(report(player_bursts=0, side1_bursts=0, hits=0), {"fight"}),
            ["player never fired", "side1 never fired"],
        )

    def test_a_parked_computer_fails_the_patrol(self):
        # The armed-volcano regression: the computer parked off a volcano and sailed nowhere.
        self.assertEqual(
            evaluate(report(sailed=0.0), {"patrol"}),
            ["Ship_Pirate_Barque sailed only 0 units"],
        )
        self.assertEqual(
            evaluate(report(sailed=0.0), {"fight"}),
            [],
            "a fight does not need the computer to sail",
        )

    def test_grinding_on_scenery_always_fails(self):
        self.assertEqual(
            evaluate(report(contact=7.5), set()),
            ["Ship_Pirate_Barque (side1) ground on scenery for 7.5 s"],
        )

    def test_late_first_shot(self):
        self.assertEqual(
            evaluate(report(first=95.0), {"fight"}),
            ["the computer first fired at 95 s"],
        )


class Outcome(unittest.TestCase):
    def test_reads_the_report_line(self):
        log = (
            "INFO openprocyon::smoke::report: battle report "
            + json.dumps(report())
            + "\n"
        )
        passed, summary = outcome(log, {"fight"})
        self.assertTrue(passed, summary)
        self.assertIn("side1: 5 bursts 3 hits", summary)

    def test_panic_and_missing_report(self):
        self.assertEqual(
            outcome("thread 'main' panicked at src/x.rs:1", set()), (False, "PANIC")
        )
        self.assertFalse(outcome("", set())[0])


if __name__ == "__main__":
    unittest.main()

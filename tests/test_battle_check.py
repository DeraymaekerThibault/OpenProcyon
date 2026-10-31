import json
import unittest

from tools.battle_check import SCENARIOS, evaluate, outcome


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
            ["Ship_Pirate_Barque (side1) sailed only 0 units"],
        )
        self.assertEqual(
            evaluate(report(sailed=0.0), {"fight"}),
            [],
            "a fight does not need the computer to sail",
        )
        sunk = report(sailed=0.0)
        for s in sunk["ships"]:
            s["sunk"] = True
        self.assertEqual(
            evaluate(sunk, {"patrol"}), [], "a ship sunk early is not parked"
        )

    def test_grinding_on_scenery_always_fails(self):
        self.assertEqual(
            evaluate(report(contact=7.5), set()),
            ["Ship_Pirate_Barque (side1) ground on scenery for 7.5 s"],
        )

    def test_stutter_fails(self):
        # Border Dispute before the lazy planner: a 0.5 s route plan every few seconds.
        r = report()
        r["frame_ms"] = {"p50": 16.0, "p99": 525.0, "worst": 535.0, "over_50": 29}
        self.assertEqual(
            evaluate(r, set()), ["stutter: 29 frames over 50 ms (worst 535 ms)"]
        )
        self.assertEqual(evaluate(r, {"no-stutter"}), [], "only an open issue skips it")

    def test_late_first_shot(self):
        self.assertEqual(
            evaluate(report(first=95.0), {"fight"}),
            ["the computer first fired at 95 s"],
        )


def lobby_report(ally_sailed=600.0, ally_bursts=4, foe_first=40.0, spent=(900, 900)):
    """A full Border Dispute lobby: the human's own ship, one allied computer, one hostile computer."""
    ship = lambda cls, side, computer, sailed, bursts: {  # noqa: E731
        "class": cls,
        "side": side,
        "computer": computer,
        "sailed": sailed,
        "bursts": bursts,
        "worst_contact": 0.0,
    }
    return {
        "time": 300.0,
        "sides": {
            "player": {"ships": 2, "alive": 2, "bursts": 9, "hits": 3, "first_burst": 20.0},
            "side1": {"ships": 1, "alive": 1, "bursts": 5, "hits": 3, "first_burst": foe_first},
        },
        "ships": [
            ship("Ship_Navy_Cutter", "player", False, 0.0, 5),
            ship("Ship_Navy_Frigate", "player", True, ally_sailed, ally_bursts),
            ship("Ship_Pirate_Barque", "side1", True, 800.0, 5),
        ],
        "fleets": [
            {"player": 0, "side": "player", "human": True, "ships": 1, "spent": 1000, "budget": 1000, "limit": 10},
            {"player": 1, "side": "player", "human": False, "ships": 5, "spent": spent[0], "budget": 1000, "limit": 10},
            {"player": 2, "side": "side1", "human": False, "ships": 5, "spent": spent[1], "budget": 1000, "limit": 10},
        ],
    }  # fmt: skip


class LobbyExpectations(unittest.TestCase):
    expect = frozenset({"patrol", "fire", "budget"})

    def test_a_healthy_full_lobby_passes(self):
        self.assertEqual(evaluate(lobby_report(), self.expect), [])

    def test_an_allied_computer_that_never_moves_fails(self):
        # The regression the --allies shortcut missed: allied computers shared the human's side and sat still.
        r = lobby_report(ally_sailed=0.0, ally_bursts=0)
        self.assertEqual(
            evaluate(r, self.expect),
            [
                "no computer ship of player fired",
                "Ship_Navy_Frigate (player) sailed only 0 units",
            ],
        )

    def test_the_humans_own_fleet_may_wait_for_orders(self):
        # The human's cutter sails 0 units in every report above and that is fine.
        self.assertEqual(evaluate(lobby_report(), {"patrol"}), [])

    def test_a_side_that_fires_late_or_never_fails(self):
        self.assertEqual(
            evaluate(lobby_report(foe_first=400.0), {"fire"}),
            ["side1 first fired at 400 s"],
        )
        r = lobby_report()
        r["sides"]["side1"]["first_burst"] = None
        self.assertEqual(evaluate(r, {"fire"}), ["side1 never fired"])

    def test_a_fleet_that_leaves_its_budget_unspent_fails(self):
        # 1000 points and ten ships, and the computer bought a few cheap hulls.
        self.assertEqual(
            evaluate(lobby_report(spent=(300, 900)), {"budget"}),
            ["player computer fleet (player 1) spent 300 of 1000 points"],
        )

    def test_the_humans_fleet_is_not_judged_on_budget(self):
        r = lobby_report()
        r["fleets"][0]["spent"] = 10
        self.assertEqual(evaluate(r, {"budget"}), [])

    def test_reports_without_the_new_fields_still_evaluate(self):
        # Old reports: no `computer`, no `bursts`, no `fleets`; the player side counts as the human's.
        self.assertEqual(evaluate(report(), {"patrol", "budget"}), [])

    def test_the_owners_game_is_a_scenario_with_allied_slots(self):
        by_name = {s.name: s for s in SCENARIOS}
        s = by_name["border_dispute_full"]
        self.assertEqual(s.args[0], "--lobby")
        for part in (
            "map=border_dispute",
            "points=1000",
            "limit=10",
            "slots=human,7*computer",
        ):
            self.assertIn(part, s.args[1])
        self.assertLessEqual({"patrol", "fire"}, s.expect)


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

    def test_the_summary_shows_computer_fleet_spending(self):
        log = "battle report " + json.dumps(lobby_report()) + "\n"
        passed, summary = outcome(log, {"patrol", "fire"})
        self.assertTrue(passed, summary)
        self.assertIn("computer fleets spent 1800/2000 points", summary)

    def test_panic_and_missing_report(self):
        self.assertEqual(
            outcome("thread 'main' panicked at src/x.rs:1", set()), (False, "PANIC")
        )
        self.assertFalse(outcome("", set())[0])


if __name__ == "__main__":
    unittest.main()

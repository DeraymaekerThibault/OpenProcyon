import json
import unittest

from tools.battle_check import (
    FIRE_BY_FOR,
    SCENARIOS,
    evaluate,
    outcome,
    replay_args,
    same_run,
)


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

    def test_engagement_does_not_require_an_uncommanded_human_to_fire(self):
        r = report(player_bursts=0)
        r["sides"]["player"]["hits"] = 0
        self.assertEqual(evaluate(r, {"engage"}), [])
        for name in ("ambush", "bayles_stand"):
            scenario = next(s for s in SCENARIOS if s.name == name)
            self.assertIn("engage", scenario.expect)
            self.assertNotIn("fight", scenario.expect)

    def test_engagement_still_requires_computer_fire_hits_and_promptness(self):
        self.assertEqual(
            evaluate(report(side1_bursts=0), {"engage"}), ["side1 never fired"]
        )
        self.assertEqual(evaluate(report(hits=0), {"engage"}), ["side1 never hit"])
        self.assertEqual(
            evaluate(report(first=95.0), {"engage"}),
            ["the computer first fired at 95 s"],
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
            evaluate(report(contact=12.5), set()),
            ["Ship_Pirate_Barque (side1) ground on scenery for 12.5 s"],
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

    def test_border_dispute_full_alone_has_the_longer_fire_limit(self):
        late = lobby_report(foe_first=300.0)
        self.assertEqual(evaluate(late, {"fire"}), ["side1 first fired at 300 s"])
        self.assertEqual(evaluate(late, {"fire"}, FIRE_BY_FOR["border_dispute_full"]), [])
        self.assertEqual(list(FIRE_BY_FOR), ["border_dispute_full"])

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


class Invariants(unittest.TestCase):
    def test_non_finite_positions_far_ships_and_runaway_entities_always_fail(self):
        r = report()
        r["sanity"] = {"non_finite": 2, "farthest": 90000, "entities": 70000}
        self.assertEqual(
            evaluate(r, set()),
            [
                "2 ship positions were not finite (NaN or infinity)",
                "a ship got 90000 units from the origin (limit 50000)",
                "70000 entities alive at once (limit 50000)",
            ],
        )

    def test_a_healthy_run_and_an_old_report_pass_the_sanity_check(self):
        r = report()
        self.assertEqual(evaluate(r, set()), [], "reports without a sanity block")
        r["sanity"] = {"non_finite": 0, "farthest": 4000, "entities": 3000}
        self.assertEqual(evaluate(r, set()), [])

    def test_resolve_wants_an_end_or_a_sinking(self):
        r = report()
        r["ended"] = False
        self.assertEqual(
            evaluate(r, {"resolve"}), ["nothing sank and no side was beaten in 120 s"]
        )
        self.assertEqual(evaluate(r, set()), [], "only where asked for")
        r["ships"][1]["sunk"] = True
        self.assertEqual(evaluate(r, {"resolve"}), [])
        r["ships"][1]["sunk"] = False
        r["ended"] = True
        self.assertEqual(evaluate(r, {"resolve"}), [])

    def test_the_same_seed_must_give_the_same_report(self):
        def log(frame_ms, hits=3):
            r = report(hits=hits)
            r["frame_ms"] = {"p50": frame_ms, "over_50": 0}
            return "battle report " + json.dumps(r)

        self.assertEqual(
            same_run(log(16), log(23))[0], True, "frame times differ per run"
        )
        same, why = same_run(log(16), log(16, hits=4))
        self.assertFalse(same)
        self.assertIn("sides", why)
        self.assertFalse(same_run(log(16), "crashed")[0])

    def test_more_maps_and_mixed_fleets_are_scenarios(self):
        names = {s.name for s in SCENARIOS}
        self.assertTrue({"red_rover", "mixed_fleets"} <= names)

    def test_a_lobby_scenario_replays_from_the_log_alone(self):
        self.assertEqual(
            replay_args(["--lobby", "map=x;slots=human", "--seed", "2"]),
            ["--seed", "2"],
        )
        self.assertEqual(replay_args(["--map", "red_rover"]), ["--map", "red_rover"])

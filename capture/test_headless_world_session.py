"""Behavioral checks for the read-only zero-observer acceptance evaluator."""
import copy
import unittest
from headless_world_session import reject_remote_success

from headless_world_session import HeadlessAcceptance


def fixture():
    extent = 4096 * .2 ** .5
    people = [{'id': i, 'home': None, 'wallet': 1000} for i in range(1, 25)]
    towns = [{'id': i, 'resident_count_reported': 6,
              'resident_ids': list(range((i - 1) * 6 + 1, i * 6 + 1)),
              'tier': 'Hamlet', 'buildings': [], 'position': [i * 500., 0., 0.],
              'stock': {'Bread': 20, 'Wheat': 6, 'Wood': 12}, 'treasury': 2300}
             for i in range(1, 5)]
    opening = {'settlement_count': 4, 'founder_count': 24, 'coastal_gateways': 1,
               'bounds': {'min': [-extent, -extent], 'max': [extent, extent]},
               'settlements': towns, 'people': people,
               'accounting': {'money_total': 33200, 'new_villager_endowment': 1000}}
    row = {'day': 0, 'absolute_world_seconds': 200., 'cycle_duration': 1440,
           'observer_clients': 0, 'observed_regions': 0, 'boat_entries': 0,
           'incoming_boats': [], 'events': [], 'settlements': copy.deepcopy(towns),
           'people': [dict(person, resident_of=(person['id'] - 1) // 6 + 1,
                           nutrition={'total_meals': 1}) for person in people],
           'accounting': {'money_total': 33200, 'businesses': [{
               'id': 5, 'account': {'current_day': {'day': 0, 'produced_units': 3},
                                  'previous_day': {'day': 2**32 - 1, 'produced_units': 0}}}]}}
    for town in row['settlements']:
        town.update(houses=1, businesses=1, ever_observed=False, observer_count=0)
    return opening, row


class HeadlessEvaluatorTests(unittest.TestCase):
    def test_never_observed_growth_and_ledger_deduplication(self):
        opening, row = fixture()
        tracker = HeadlessAcceptance(opening, 0)
        tracker.observe(row)
        tracker.observe(dict(row, absolute_world_seconds=210.))
        tracker.finish()
        self.assertEqual(tracker.evidence()['observed_site_day_production_units'], 3)

    def test_a_brief_past_observation_cannot_pass_as_unobserved(self):
        opening, row = fixture()
        row['settlements'][2]['ever_observed'] = True
        with self.assertRaises(AssertionError):
            HeadlessAcceptance(opening, 0).observe(row)

    def test_authored_views_must_reach_each_real_town_region(self):
        opening, row = fixture()
        row['observation'] = {'mode': 'all', 'active': True, 'view_slots': 4}
        row['observer_clients'] = 4
        row['observed_regions'] = 9
        for town in row['settlements']:
            town.update(observer_count=1, ever_observed=True)
        tracker = HeadlessAcceptance(opening, 0, 'all')
        tracker.observe(row)
        tracker.finish()
        row['settlements'][2]['observer_count'] = 0
        with self.assertRaises(AssertionError):
            HeadlessAcceptance(opening, 0, 'all').observe(row)

    def test_alternating_requires_both_actual_observation_states(self):
        opening, row = fixture()
        row['observation'] = {'mode': 'alternating', 'active': False, 'view_slots': 4}
        tracker = HeadlessAcceptance(opening, 0, 'alternating')
        tracker.observe(row)
        with self.assertRaises(AssertionError):
            tracker.finish()
        row['observation']['active'] = True
        row['observer_clients'] = 4
        row['observed_regions'] = 9
        for town in row['settlements']:
            town.update(observer_count=1, ever_observed=True)
        tracker.observe(row)
        tracker.finish()

    def test_cash_loss_is_not_hidden_by_a_changed_baseline(self):
        opening, row = fixture()
        row['accounting']['money_total'] -= 1
        with self.assertRaises(AssertionError):
            HeadlessAcceptance(opening, 0).observe(row)

    def test_growth_in_one_town_does_not_stand_in_for_other_towns(self):
        opening, row = fixture()
        row['settlements'][2]['houses'] = 0
        tracker = HeadlessAcceptance(opening, 0)
        tracker.observe(row)
        with self.assertRaises(AssertionError):
            tracker.finish()


if __name__ == '__main__':
    unittest.main()


class RemoteSuccessChecks(unittest.TestCase):
    def test_old_remote_success_cannot_pass_on_totals(self):
        for line in ('Farmer Ada completed a loaded workplace handoff abstractly after 3 failed routes',
                     'Moot food purchase completing at the counter fallback'):
            with self.assertRaises(AssertionError):
                reject_remote_success(line)
        reject_remote_success('A route failed; cargo retained for physical retry')

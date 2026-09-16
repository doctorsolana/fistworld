"""Evidence-parser checks; these do not replace the connected game run."""
import copy
import unittest

from small_world_session import Acceptance, check_unvisited_towns


def opening():
    towns = []
    people = []
    for town in range(1, 5):
        ids = list(range((town-1)*6+1, town*6+1))
        towns.append({'id': town, 'resident_count_reported': 6, 'resident_ids': ids,
                      'tier': 'Hamlet', 'buildings': [], 'position': [(town-1)*500, 2, 0],
                      'stock': {'Bread': 20, 'Wheat': 6, 'Wood': 12}, 'treasury': 2300})
        people.extend({'id': person, 'home': None, 'wallet': 1000} for person in ids)
    extent = 4096 * .2**.5
    return {'settlement_count': 4, 'founder_count': 24, 'settlements': towns,
            'people': people, 'coastal_gateways': 1,
            'bounds': {'min': [-extent, -extent], 'max': [extent, extent]}}


def sample(person=25, now=10, chosen=None, resident=None):
    return {'absolute_world_seconds': now, 'settlements': opening()['settlements'],
            'incoming_boats': [], 'events': [], 'people': [{
                'id': person, 'aboard': resident is None, 'voyage': resident is None,
                'immigration_queue': False, 'resident_of': resident, 'chosen_town': chosen,
                'counts_as_resident': resident is not None, 'immigration_departure': False,
                'entered_at': 10+(person-25)*20, 'chosen_at': 11+(person-25)*20 if chosen else None,
                'chosen_score': 34 if chosen else None, 'entry': [0, 0, -40]}]}


class SmallWorldEvidenceTests(unittest.TestCase):
    def test_camera_town_construction_cannot_stand_in_for_unvisited_towns(self):
        row = sample()
        for town in row['settlements']:
            town['ever_observed'] = town['id'] == 1
        row['settlements'][0]['buildings'] = [{'kind': 'House'}]
        with self.assertRaises(AssertionError):
            check_unvisited_towns(row, 1)
        for town in row['settlements'][1:]:
            town['buildings'] = [{'kind': 'House'}]
        self.assertEqual(len(check_unvisited_towns(row, 1)['unvisited_towns']), 3)
        row['settlements'][1]['ever_observed'] = True
        row['settlements'][2]['ever_observed'] = True
        with self.assertRaises(AssertionError):
            check_unvisited_towns(row, 1)

    def test_valid_boat_choice_registration_can_all_favour_one_town(self):
        acceptance = Acceptance(opening(), 3)
        for person in (25, 26, 27):
            acceptance.observe(sample(person, now=10+(person-25)*20))
            acceptance.observe(sample(person, now=11+(person-25)*20, chosen=1))
            acceptance.observe(sample(person, now=20+(person-25)*20, chosen=1, resident=1))
        self.assertTrue(acceptance.passed())
        self.assertEqual(acceptance.evidence()['arrivals_by_town'], {1: 3, 2: 0, 3: 0, 4: 0})

    def test_a_spawned_boat_is_not_a_registered_arrival(self):
        acceptance = Acceptance(opening(), 1)
        acceptance.observe(sample())
        acceptance.observe(sample(now=11, chosen=1))
        self.assertFalse(acceptance.passed())
        with self.assertRaises(AssertionError):
            Acceptance(opening(), 1).observe(sample(now=20, chosen=1, resident=1))

    def test_walking_and_counter_departure_do_not_grant_citizenship(self):
        acceptance = Acceptance(opening(), 1)
        acceptance.observe(sample())
        acceptance.observe(sample(now=11, chosen=1))
        row = sample(now=20, chosen=1)
        row['people'][0].update(aboard=False, voyage=False, immigration_queue=True)
        acceptance.observe(row)
        self.assertFalse(acceptance.passed(), 'walking to the Hall is not registration')
        row['absolute_world_seconds'] = 21
        row['people'][0].update(immigration_queue=False, immigration_departure=True)
        acceptance.observe(row)
        self.assertFalse(acceptance.passed(), 'counter departure must physically finish')
        row['absolute_world_seconds'] = 22
        row['people'][0].update(immigration_departure=False, counts_as_resident=True, resident_of=1)
        acceptance.observe(row)
        self.assertTrue(acceptance.passed())

    def test_early_citizenship_housing_and_employment_fail(self):
        for field in ('resident_of', 'home', 'workplace'):
            with self.subTest(field=field):
                acceptance = Acceptance(opening(), 1)
                acceptance.observe(sample())
                row = sample(now=11, chosen=1)
                row['people'][0][field] = 1
                with self.assertRaises(AssertionError):
                    acceptance.observe(row)

    def test_incoming_stages_cannot_count_as_registered_population(self):
        for stage in ('aboard', 'voyage', 'immigration_queue', 'immigration_departure'):
            with self.subTest(stage=stage):
                acceptance = Acceptance(opening(), 1)
                acceptance.observe(sample())
                row = sample(now=11, chosen=1)
                row['people'][0].update(aboard=False, voyage=False, counts_as_resident=True)
                row['people'][0][stage] = True
                with self.assertRaises(AssertionError):
                    acceptance.observe(row)

    def test_preselected_destination_and_out_of_bounds_route_fail(self):
        acceptance = Acceptance(opening(), 1)
        row = sample(now=11, chosen=1)
        row['people'][0]['chosen_at'] = 10
        with self.assertRaises(AssertionError):
            acceptance.observe(row)
        row = sample()
        row['incoming_boats'] = [{'position_in_bounds': True, 'route_in_bounds': False,
                                  'cursor_valid': True, 'route_points': 4}]
        with self.assertRaises(AssertionError):
            acceptance.observe(row)
        bad = copy.deepcopy(opening())
        bad['settlements'][0]['stock']['Bread'] = 2000
        with self.assertRaises(AssertionError):
            Acceptance(bad, 1)


if __name__ == '__main__':
    unittest.main()

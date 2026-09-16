"""Evidence-observer tests, not connected movement or meal-system acceptance."""
import unittest

from construction_meals import ConstructionMeals


SITE = {'entity': '91v3', 'owner': None, 'stand': [10, 0, 0], 'raising': True}


def row(objective='ConstructingBuilding', activity='Building', x=10, person=4,
        assignment='7v0', elapsed=1):
    return {'elapsed': elapsed, 'actors': [{
        'id': person, 'position': [x, 0, 0], 'objective': f'Some({objective})',
        'activity': f'Some({activity})',
        'intent': f'Some(Building {{ settlement: 2v0, site: {assignment} }})',
    }]}


class ConstructionMealTests(unittest.TestCase):
    def test_same_person_finishes_food_and_returns_to_the_original_physical_stand(self):
        observer = ConstructionMeals()
        events = []
        for sample in [row(), row('QueuedForPersonalFood', 'Idle', 20, elapsed=2),
                       row('Eating', 'Sitting', 25, elapsed=3), row(x=25, elapsed=4)]:
            events += observer.observe(sample, [SITE])
        self.assertEqual([event['event'] for event in events], ['construction', 'food'])
        self.assertFalse(observer.report()['passed'], 'remote Building is not resumed site work')
        resumed = observer.observe(row(elapsed=5), [SITE])
        self.assertEqual(resumed[0]['event'], 'resumed')
        self.assertEqual(resumed[0]['actor']['id'], 4)
        self.assertEqual(resumed[0]['assignment'], '7v0')
        self.assertEqual(resumed[0]['site']['entity'], '91v3')
        self.assertTrue(observer.report()['passed'])

    def test_queue_only_no_displacement_and_other_person_never_supply_the_missing_meal(self):
        for food in [row('QueuedForPersonalFood', 'Idle', 25),
                     row('Eating', 'Sitting', 10),
                     row('Eating', 'Sitting', 25, person=5)]:
            with self.subTest(food=food):
                observer = ConstructionMeals()
                for sample in [row(), food, row()]:
                    observer.observe(sample, [SITE])
                self.assertFalse(observer.report()['passed'])

    def test_assignment_change_ambiguous_stand_and_remote_work_cannot_bind(self):
        observer = ConstructionMeals()
        for sample in [row(), row('Eating', 'Sitting', 25), row(assignment='8v0'), row()]:
            observer.observe(sample, [SITE])
        self.assertFalse(observer.report()['passed'])
        for sites, initial in [([SITE, dict(SITE, entity='99v0')], row()),
                               ([SITE], row(x=25))]:
            observer = ConstructionMeals()
            for sample in [initial, row('Eating', 'Sitting', 25), row()]:
                observer.observe(sample, sites)
            self.assertFalse(observer.report()['passed'])


if __name__ == '__main__':
    unittest.main()

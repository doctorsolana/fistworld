"""The connected acceptance must reject remote food and hidden land walkers."""
import copy
import unittest

from port_trade_session import Acceptance


class CaptainAcceptanceTests(unittest.TestCase):
    def setUp(self):
        self.acceptance = Acceptance({'home': {'shore': [0, 0, 10]}, 'cargo': 12})
        self.row = {
            'money': 100, 'initial_money': 100,
            'goods': [72, 8, 12], 'initial_goods': [72, 8, 12],
            'ships': [],
            'people': [{
                'id': 1, 'position': [0, 0, 0], 'activity': 'Idle',
                'haul': False, 'builder': False, 'aboard': None,
                'crew': 'CollectingProvisions', 'food': 0,
                'provision_counter': [0, 0, 0],
                'workplace_interior': False, 'workplace_door_transit': False,
            }],
        }

    def test_actual_counter_purchase_survives_clearing_the_completed_pickup(self):
        self.acceptance.observe(self.row)
        after = copy.deepcopy(self.row)
        after['people'][0].update(food=3, crew='Approaching', provision_counter=None)
        self.acceptance.observe(after)
        self.assertTrue(self.acceptance.provisions_collected)

    def test_food_added_at_the_berth_cannot_pass_as_provisioning(self):
        self.acceptance.observe(self.row)
        after = copy.deepcopy(self.row)
        after['people'][0].update(food=3, position=[0, 0, 10], crew='Aboard')
        with self.assertRaisesRegex(AssertionError, 'remote captain provisions'):
            self.acceptance.observe(after)

    def test_land_walker_cannot_keep_an_indoor_animation(self):
        self.row['people'][0]['activity'] = 'Indoors'
        with self.assertRaisesRegex(AssertionError, 'indoor/work animation'):
            self.acceptance.observe(self.row)


if __name__ == '__main__':
    unittest.main()

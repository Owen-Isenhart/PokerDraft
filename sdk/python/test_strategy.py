import unittest
from pokerdraft import evaluate, best_hand

class StrategyTest(unittest.TestCase):
    def test_wheel_and_kickers(self):
        self.assertEqual(evaluate([12, 13, 27, 41, 3]), ((4, 5), 55))
        self.assertGreater(evaluate([12, 25, 11, 10, 9])[0], evaluate([11, 24, 12, 10, 9])[0])
    def test_royal_and_best_selection(self):
        self.assertEqual(evaluate([8, 9, 10, 11, 12])[1], 600)
        self.assertEqual(best_hand([0, 1, 8, 9, 10, 11, 12])[0], [8, 9, 10, 11, 12])
    def test_not_enough_cards(self):
        self.assertIsNone(best_hand([0, 1, 2, 3]))

if __name__ == '__main__':
    unittest.main()

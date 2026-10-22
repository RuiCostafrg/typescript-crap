import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('mutation_score', Path(__file__).resolve().parents[1] / 'scripts/mutation-score.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class MutationScoreTests(unittest.TestCase):
    def report(self, caught=9, missed=1, timeout=0, unviable=0):
        return {'outcomes': [{'scenario':'Baseline','summary':'Success'}] + [
            {'scenario':{'Mutant':{}},'summary':summary}
            for summary, count in [('CaughtMutant',caught),('MissedMutant',missed),('Timeout',timeout),('Unviable',unviable)]
            for _ in range(count)], 'total_mutants':caught+missed+timeout+unviable, 'end_time':'done'}
    def test_exact_floor_and_unviable_not_inflating_score(self):
        self.assertEqual(module.check(self.report(unviable=100)),90)
        with self.assertRaises(ValueError): module.check(self.report(caught=8,missed=2,unviable=100))
    def test_timeout_empty_partial_and_bad_baseline_fail(self):
        for report in [self.report(caught=0,missed=0), self.report(caught=99,missed=0,timeout=1),
                       {'outcomes':[]}, {**self.report(),'end_time':None}, {**self.report(),'total_mutants':11}]:
            with self.assertRaises(ValueError): module.check(report)
        report=self.report(); report['outcomes'][0]['summary']='Failure'
        with self.assertRaises(ValueError): module.check(report)

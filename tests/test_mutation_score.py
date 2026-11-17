import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
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
    def shard(self, caught=0, missed=0, timeout=0, unviable=0, offset=0):
        outcomes=[{'scenario':'Baseline','summary':'Success'}]
        for summary,count in [('CaughtMutant',caught),('MissedMutant',missed),('Timeout',timeout),('Unviable',unviable)]:
            for _ in range(count):
                outcomes.append({'scenario':{'Mutant':{'name':f'm{offset}'}},'summary':summary})
                offset+=1
        return {'outcomes':outcomes,'total_mutants':caught+missed+timeout+unviable,'end_time':'done'}
    def expected(self, *reports):
        return [{'name':o['scenario']['Mutant']['name']} for r in reports for o in r['outcomes'] if o['scenario']!='Baseline']
    def test_shards_combine_to_pass(self):
        a,b=self.shard(caught=9),self.shard(missed=1,offset=9)
        self.assertEqual(module.check_all([a,b],expected=self.expected(a,b)),90)
        self.assertEqual(module.check_all([a,b]),90)
    def test_floor_applies_to_combined_score(self):
        a,b=self.shard(caught=10),self.shard(caught=7,missed=3,offset=10)
        self.assertEqual(module.check_all([a],minimum=100),100)
        with self.assertRaises(ValueError): module.check_all([a,b],expected=self.expected(a,b))
    def test_timeout_in_any_shard_fails(self):
        a,b=self.shard(caught=9),self.shard(caught=9,timeout=1,offset=9)
        with self.assertRaises(ValueError): module.check_all([a,b],expected=self.expected(a,b))
    def test_expected_rejects_missing_duplicate_and_foreign_shards(self):
        a,b=self.shard(caught=9),self.shard(missed=1,offset=9)
        with self.assertRaises(ValueError): module.check_all([a],expected=self.expected(a,b))
        with self.assertRaises(ValueError): module.check_all([a,a],expected=self.expected(a))
        with self.assertRaises(ValueError): module.check_all([a,self.shard(caught=1,offset=99)],expected=self.expected(a))
    def test_any_incomplete_shard_fails_whole_check(self):
        a,b=self.shard(caught=9),self.shard(missed=1,offset=9)
        for bad in [{**b,'end_time':None},{**b,'total_mutants':2}]:
            with self.assertRaises(ValueError): module.check_all([a,bad],expected=self.expected(a,b))
        bad=self.shard(missed=1,offset=9); bad['outcomes'][0]['summary']='Failure'
        with self.assertRaises(ValueError): module.check_all([a,bad],expected=self.expected(a,b))
    def test_empty_reports_and_expected_fail(self):
        with self.assertRaises(ValueError): module.check_all([])
        with self.assertRaises(ValueError): module.check_all([self.shard(caught=9)],expected=[])
    def test_cli_scores_multiple_shards_and_rejects_missing_shard(self):
        a,b=self.shard(caught=9),self.shard(missed=1,offset=9)
        script=str(Path(__file__).resolve().parents[1]/'scripts/mutation-score.py')
        with tempfile.TemporaryDirectory() as directory:
            paths=[]
            for name,payload in [('a.json',a),('b.json',b),('list.json',self.expected(a,b))]:
                path=Path(directory)/name
                path.write_text(json.dumps(payload),encoding='utf-8')
                paths.append(str(path))
            run=subprocess.run([sys.executable,script,'--expected',paths[2],paths[0],paths[1]],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stderr)
            self.assertIn('Mutation score: 90.00%',run.stdout)
            old=subprocess.run([sys.executable,script,paths[0]],capture_output=True,text=True)
            self.assertEqual(old.returncode,0,old.stderr)
            missing=subprocess.run([sys.executable,script,'--expected',paths[2],paths[0]],capture_output=True,text=True)
            self.assertNotEqual(missing.returncode,0)

#!/usr/bin/env python3
"""Verify the documented baseline/PR workflow using installed helper bytes."""
import argparse
import hashlib
import json
import shutil
from pathlib import Path
import subprocess
import tempfile
import zipfile


def run(args, cwd, expected=0):
    result=subprocess.run([str(a) for a in args],cwd=cwd,capture_output=True,text=True,timeout=90)
    if result.returncode!=expected:
        raise RuntimeError(f'{args}: expected {expected}, got {result.returncode}\n{result.stdout}\n{result.stderr}')
    return result.stdout


def verify(archive, scanner, vitest=None, output=None):
    archive,scanner=archive.resolve(),scanner.resolve()
    fields=Path(str(archive)+'.sha256').read_text().split()
    if len(fields)!=2 or fields[1].lstrip('*')!=archive.name or fields[0]!=hashlib.sha256(archive.read_bytes()).hexdigest():
        raise ValueError('Invalid helper archive checksum')
    with tempfile.TemporaryDirectory(prefix='ts-crap CI guide ü ') as temporary:
        root=Path(temporary).resolve(); project=root/'checkout';project.mkdir()
        with zipfile.ZipFile(archive) as bundle:
            names=set()
            for entry in bundle.infolist():
                parts=entry.filename.split('/')
                if (len(parts)!=2 or parts[0]!='ts-crap-coverage-helpers' or parts[1] in ('','.','..')
                    or '\\' in entry.filename or ':' in entry.filename or entry.filename in names
                    or (entry.external_attr>>16)&0o170000!=0o100000):
                    raise ValueError('Invalid helper archive member')
                names.add(entry.filename)
            bundle.extractall(root)
        helpers=root/'ts-crap-coverage-helpers'
        version=json.loads((helpers/'version.json').read_text())['scannerVersion']
        if run([scanner,'--version'],root).strip()!='ts-crap '+version: raise ValueError('Helper/scanner version mismatch')
        run(['git','init','--quiet',project],root)
        run(['git','config','user.name','CI guide verification'],project)
        run(['git','config','user.email','ci-guide@example.invalid'],project)
        source=project/'src'/'price.mts';source.parent.mkdir()
        original='export function price(q: number) {\n  if (q > 0) return q * 4;\n  return 0;\n}\n'
        source.write_bytes(original.encode())
        (project/'test.mjs').write_text("import {price} from './src/price.mts'; if(price(1)!==4) throw Error('incorrect price');\n")
        if vitest:
            vitest=vitest.resolve()
            (project/'node_modules').symlink_to(vitest.parent.parent,target_is_directory=True)
            (project/'test.mjs').write_text("import {test,expect} from 'vitest'; import {price} from './src/price.mts'; test('price',()=>expect(price(1)).toBe(4));\n")
            (project/'vitest.config.mjs').write_text("import {defineConfig} from 'vitest/config'; export default defineConfig({test:{include:['test.mjs'],maxWorkers:1,coverage:{provider:'istanbul',include:['src/**/*.mts']}}});\n")
        (project/'ts-crap.json').write_text(json.dumps({'version':1,'include':['src/**'],'threshold':2,'require_coverage':True,'fail_on_empty':False}))
        run(['git','add','.'],project);run(['git','commit','--quiet','-m','Base source and policy'],project)
        counter=0
        def collect():
            nonlocal counter
            counter+=1; directory=root/f'run-{counter}';directory.mkdir()
            native=directory/'native.json'
            if vitest:
                run(['node',helpers/'import-coverage.mjs','--root',project,'--format','istanbul',
                     '--input',directory/'raw/coverage-final.json','--output',native,'--','node',vitest,
                     'run','--config','vitest.config.mjs','--coverage','--coverage.provider=istanbul',
                     '--coverage.reporter=json',f'--coverage.reportsDirectory={directory / "raw"}'],root)
            else:
                run(['node',helpers/'coverage.mjs','--root',project,'--output',native,'--','node','test.mjs'],root)
            return directory,native
        directory,native=collect()
        baseline=project/'crap-baseline.json'
        # Manual baseline creation still returns the absolute gate result.
        run([scanner,'--root',project,'--coverage',native,'--write-baseline',baseline],root,1)
        captured=baseline.read_bytes()
        run(['git','add','crap-baseline.json'],project);run(['git','commit','--quiet','-m','Reviewed initial baseline'],project)
        base=run(['git','rev-parse','HEAD'],project).strip()
        def gate(directory,native,expected):
            common=[scanner,'--root',project,'--coverage',native,'--baseline',baseline,'--diff',base]
            for format in ['json','sarif']:
                run([*common,'--format',format,'--output',directory/f'crap.{format}'],root,expected)
            report=json.loads((directory/'crap.json').read_text())
            sarif=json.loads((directory/'crap.sarif').read_text())
            assert report['failed']==bool(expected)
            assert sarif['version']=='2.1.0'
            return report,sarif
        assert gate(directory,native,0)[0]['files_scanned']==0
        source.write_bytes((original+'// harmless selected change\n').encode())
        run(['git','add','src/price.mts'],project);run(['git','commit','--quiet','-m','Comment-only PR'],project)
        directory,previous=collect();report,_=gate(directory,previous,0)
        assert len(report['functions'])==1
        source.write_bytes(original.replace('  if (q > 0)', '  if (q > 10) return q * 3;\n  if (q > 0)').encode())
        run(['git','add','src/price.mts'],project);run(['git','commit','--quiet','-m','Regression PR'],project)
        directory,native=collect();report,sarif=gate(directory,native,1)
        assert report['functions'][0]['failed']
        assert any(result['ruleId']=='TSCRAP001' for result in sarif['runs'][0]['results'])
        # Failed gates retain both artifacts. Fresh collection remains repeatable.
        repeated,again=collect();gate(repeated,again,1)
        assert native.read_bytes()==again.read_bytes()
        destination=directory/'crap.json';before=destination.read_bytes()
        run([scanner,'--root',project,'--diff',base,'--coverage',previous,'--output',destination],root,2)
        assert destination.read_bytes()==before
        assert baseline.read_bytes()==captured
        if output:
            output.mkdir(parents=True,exist_ok=True)
            for name in ['crap.json','crap.sarif']:
                shutil.copyfile(directory/name,output/name)
    print('PASS installed CI guide: reviewed baseline, empty/comment/regression PRs, fresh repeats, JSON/SARIF on failure, stale evidence rejection')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive',required=True,type=Path)
    parser.add_argument('--scanner',required=True,type=Path)
    parser.add_argument('--vitest',type=Path)
    parser.add_argument('--output',type=Path)
    args=parser.parse_args();verify(args.archive,args.scanner,args.vitest,args.output)

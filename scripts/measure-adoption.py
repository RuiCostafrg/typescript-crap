#!/usr/bin/env python3
"""Reproduce source/coverage adoption on pinned independent repositories (POSIX)."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import resource
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def command(args, cwd=None, allowed=(0,)):
    out = subprocess.run([str(a) for a in args], cwd=cwd, capture_output=True, text=True, timeout=240)
    if out.returncode not in allowed:
        raise RuntimeError(f'{args}: {out.returncode}\n{out.stdout}\n{out.stderr}')
    return out


def measured(args, cwd, directory, name):
    metrics = directory / f'{name}-metrics.json'
    output = directory / f'{name}.json'
    result = command([sys.executable, __file__, '--worker', metrics, output, *args], cwd, (0, 1))
    report = json.loads(output.read_text())
    return report, {**json.loads(metrics.read_text()), 'exit_code':result.returncode}


def worker():
    metrics, output = map(Path, sys.argv[2:4])
    started = time.perf_counter()
    result = subprocess.run(sys.argv[4:], capture_output=True, timeout=180)
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    metrics.write_text(json.dumps({'elapsed_seconds':time.perf_counter()-started,
        'peak_rss_kib':usage.ru_maxrss/(1024 if sys.platform == 'darwin' else 1)}))
    output.write_bytes(result.stdout)
    if result.returncode not in (0,1):
        sys.stderr.buffer.write(result.stderr)
    raise SystemExit(result.returncode)


def smoke(project):
    name = project['name']
    if name == 'hono-service':
        return """import { expect, test } from 'vitest';
import { Hono } from '../src/hono';
test('independent service handles routes and missing resources', async () => {
 const app = new Hono(); app.get('/hello/:name', c => c.json({hello:c.req.param('name')}));
 const result = await app.request('/hello/world');
 expect(result.status).toBe(200); expect(await result.json()).toEqual({hello:'world'});
 expect((await app.request('/missing')).status).toBe(404);
});
"""
    if name == 'tanstack-monorepo':
        return """import { expect, test } from 'vitest';
import React from 'react'; import { renderToStaticMarkup } from 'react-dom/server';
import { useReactTable, getCoreRowModel } from '../packages/react-table/src/index';
test('React workspace uses the actual shared core package', () => {
 function Example() {
  const table=useReactTable({data:[{value:2}],columns:[{accessorKey:'value'}],getCoreRowModel:getCoreRowModel()});
  return React.createElement('output',null,String(table.getRowModel().rows[0].getValue('value')));
 }
 expect(renderToStaticMarkup(React.createElement(Example))).toBe('<output>2</output>');
});
"""
    return """import { expect, test } from 'vitest';
import React from 'react'; import { renderToStaticMarkup } from 'react-dom/server';
import App from '../packages/create-vite/template-react-ts/src/App';
test('real generated TSX application renders with its own assets and hooks', () => {
 const html=renderToStaticMarkup(React.createElement(App));
 expect(html).toContain('Get started'); expect(html).toContain('Count is 0');
});
"""


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--scanner', type=Path, default=ROOT/'target/debug/ts-crap')
    parser.add_argument('--build-profile', choices=['debug', 'release', 'unspecified'], default='unspecified')
    parser.add_argument('--workspace',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    scanner=args.scanner.resolve(); workspace=args.workspace.resolve(); workspace.mkdir(parents=True,exist_ok=True)
    tooling=ROOT/'tests/tooling/node_modules'
    if not tooling.is_dir(): raise ValueError('Run npm ci --ignore-scripts --prefix tests/tooling first')
    projects=json.loads((ROOT/'tests/adoption/projects.json').read_text())
    results=[]
    for project in projects:
        root=workspace/project['name']
        if not root.exists():
            root.mkdir(); command(['git','init','--quiet',root])
            command(['git','-C',root,'remote','add','origin',project['repository']])
            command(['git','-C',root,'sparse-checkout','set','--no-cone',*project['sparse']])
            command(['git','-C',root,'fetch','--quiet','--depth=1','--filter=blob:none','origin',project['commit']])
            command(['git','-C',root,'checkout','--quiet','--detach','FETCH_HEAD'])
        assert command(['git','-C',root,'rev-parse','HEAD']).stdout.strip()==project['commit']
        # Only our external test/config directory and dependency link are added;
        # tracked upstream sources remain unchanged and are checked below.
        link=root/'node_modules'
        if not link.exists(): link.symlink_to(tooling,target_is_directory=True)
        helper=root/'.ts-crap-adoption'; helper.mkdir(exist_ok=True)
        (helper/'smoke.test.mjs').write_text(smoke(project))
        alias={'@tanstack/table-core':str(root/'packages/table-core/src/index.ts')} if project['name']=='tanstack-monorepo' else {}
        config='''import {defineConfig} from 'vitest/config';
export default defineConfig({esbuild:{jsx:'automatic'},resolve:{alias:ALIAS},test:{include:['.ts-crap-adoption/smoke.test.mjs'],maxWorkers:1,coverage:{provider:'istanbul',include:INCLUDE,exclude:['**/*.test.*','**/*.spec.*','**/tests/**','**/*.d.ts']}}});
'''.replace('ALIAS',json.dumps(alias)).replace('INCLUDE',json.dumps(project['coverage']))
        (helper/'vitest.config.mjs').write_text(config)
        (helper/'policy.json').write_text(json.dumps({'version':1,'threshold':10,'fail_on_empty':True}))
        directory=Path(tempfile.mkdtemp(prefix='run-',dir=helper))
        scan=[scanner,'--root',root,*project['paths'],'--config',helper/'policy.json',
              '--exclude','**/*.test.*','--exclude','**/*.spec.*','--exclude','**/tests/**','--format','json']
        uncovered, first = measured(scan,root,directory,'uncovered')
        native=directory/'native.json'; raw=directory/'raw'
        started=time.perf_counter()
        collection=command(['node',ROOT/'scripts/import-coverage.mjs','--root',root,'--format','istanbul',
            '--input',raw/'coverage-final.json','--output',native,'--','node',tooling/'vitest/vitest.mjs',
            'run','--config',helper/'vitest.config.mjs','--coverage','--coverage.reporter=json',f'--coverage.reportsDirectory={raw}'],root)
        covered, second = measured([*scan,'--coverage',native],root,directory,'covered')
        repeat=command([*scan,'--coverage',native],root,(0,1))
        assert json.loads(repeat.stdout)==covered, 'Repeated scan changed the report'
        assert covered['functions'] and any(f['coverage']>0 for f in covered['functions']), 'No executed source evidence'
        assert command(['git','-C',root,'diff','--exit-code']).returncode==0
        results.append({**project,'source_unmodified':True,'uncovered':first,'covered':second,
            'collection_and_scans_seconds':time.perf_counter()-started,'files_scanned':covered['files_scanned'],
            'functions':len(covered['functions']),'functions_missing_coverage':covered['functions_missing_coverage'],
            'files_missing_coverage':covered['files_missing_coverage'],
            'partially_covered_functions':sum(0<f['coverage']<1 for f in covered['functions']),
            'uncovered_functions':sum(f['coverage']==0 for f in covered['functions']),
            'mapping_failures':0,'repeat_identical':True,
            'highest_scores':sorted([{k:f[k] for k in ['path','name','complexity','coverage','crap']} for f in covered['functions']],key=lambda f:f['crap'],reverse=True)[:5]})
        print(f"PASS {project['name']}: {covered['files_scanned']} files, {len(covered['functions'])} functions, real Vitest JSON coverage, deterministic repeat",flush=True)
    summary={'measured_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
      'platform':platform.platform(),'node':command(['node','--version']).stdout.strip(),
      'scanner_version':command([scanner,'--version']).stdout.strip(),
      'scanner_build_profile':args.build_profile,
      'vitest':json.loads((tooling/'vitest/package.json').read_text())['version'],
      'coverage_provider':'istanbul',
      'scanner_sha256':hashlib.sha256(scanner.read_bytes()).hexdigest(),
      'rss_definition':'Peak resident set of each scanner process, measured by a fresh POSIX resource worker; KiB. Collection is wall-clock only.',
      'limits':'Targeted smoke tests exercise real source; they are not full upstream test suites or full applications. Source subsets and revisions are explicit.',
      'projects':results}
    args.output.parent.mkdir(parents=True,exist_ok=True); args.output.write_text(json.dumps(summary,indent=2)+'\n')


if __name__=='__main__':
    if len(sys.argv)>1 and sys.argv[1]=='--worker': worker()
    main()

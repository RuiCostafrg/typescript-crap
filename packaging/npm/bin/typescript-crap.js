#!/usr/bin/env node
'use strict';
// `typescript-crap coverage ...` runs the one-step Vitest/Jest coverage command;
// anything else runs the ts-crap scanner unchanged (it has no positional arguments).
const launcher = require('../lib/index.js');
const args = process.argv.slice(2);
if (args[0] === 'coverage') launcher.runCoverage(args.slice(1));
else launcher.runScanner(args);

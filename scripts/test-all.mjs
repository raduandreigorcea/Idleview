// Runs every JS test suite in the repo.
//
// The three packages are deliberately not one package: the dashboard runs in a webview,
// the photo proxy runs on Cloudflare's workerd (no DOM), and the control panel is a Vue
// app in its own repo. Each needs its own vitest environment, so each has its own config.
//
// The cost of that is a root `npm test` that used to run only the dashboard's suite and
// report green - silently skipping the worker's SSRF allowlist test and the panel's
// pairing-gate test, i.e. the two suites most worth running. This runs all three.

import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')

const suites = [
  // `test:app`, not `test` - calling the root's `test` script from here would recurse.
  { name: 'dashboard', dir: root, script: 'test:app' },
  { name: 'photo proxy (worker)', dir: join(root, 'proxy'), script: 'test' },
  // Checked out by `git submodule update --init`. A clone without it is normal, but a
  // missing submodule must read as SKIPPED, never as a pass.
  { name: 'control panel', dir: join(root, 'idleview-control'), script: 'test', optional: true },
]

const results = []

for (const suite of suites) {
  const label = `${suite.name} (${suite.dir === root ? '.' : suite.dir.slice(root.length + 1)})`

  if (!existsSync(join(suite.dir, 'package.json'))) {
    if (suite.optional) {
      console.log(`\n=== SKIP  ${label} - not checked out (git submodule update --init)\n`)
      results.push({ label, status: 'SKIP' })
      continue
    }
    console.log(`\n=== FAIL  ${label} - no package.json\n`)
    results.push({ label, status: 'FAIL' })
    continue
  }

  // An uninstalled package fails vitest with a confusing "command not found" - say what
  // is actually wrong instead.
  if (!existsSync(join(suite.dir, 'node_modules'))) {
    console.log(`\n=== FAIL  ${label} - dependencies not installed (npm --prefix "${suite.dir}" install)\n`)
    results.push({ label, status: 'FAIL' })
    continue
  }

  console.log(`\n=== RUN   ${label}\n`)
  // shell: true so this resolves npm.cmd on Windows as well as npm elsewhere.
  const run = spawnSync('npm', ['run', suite.script, '--silent'], {
    cwd: suite.dir,
    stdio: 'inherit',
    shell: true,
  })

  results.push({ label, status: run.status === 0 ? 'PASS' : 'FAIL' })
}

console.log('\n' + '='.repeat(60))
for (const result of results) {
  console.log(`${result.status.padEnd(5)} ${result.label}`)
}
console.log('='.repeat(60) + '\n')

// A skipped submodule is not a failure, but anything that actually ran and failed is.
process.exit(results.some(result => result.status === 'FAIL') ? 1 : 0)

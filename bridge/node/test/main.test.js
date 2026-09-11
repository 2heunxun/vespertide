import { describe, expect, test } from 'bun:test'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const root = join(import.meta.dir, '..')
// The addon only exists after `napi build`; without it there is nothing to test.
const built = existsSync(join(root, 'index.js'))
// Spawn `node` by name: under `bun test` `process.execPath` is Bun, and the
// package promises Node's N-API (`engines.node`).
const run = (args, cwd = root) =>
  spawnSync('node', [join(root, 'main.js'), ...args], { cwd, encoding: 'utf8' })

describe('main.js', () => {
  test.skipIf(!built)('--version exits 0 with the version line', () => {
    const result = run(['--version'])
    expect(result.status).toBe(0)
    expect(result.stdout).toMatch(/^vespertide \d/)
  })

  test.skipIf(!built)("an unknown subcommand exits 2 with clap's message, printed once", () => {
    const result = run(['bogus'])
    expect(result.status).toBe(2)
    expect(result.stderr.match(/unrecognized subcommand/g)).toHaveLength(1)
  })

  test.skipIf(!built)('a failed command exits 1 with the Error: prefix', () => {
    const dir = mkdtempSync(join(tmpdir(), 'vespertide-'))
    try {
      const result = run(['status'], dir)
      expect(result.status).toBe(1)
      expect(result.stderr).toStartWith('Error: ')
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

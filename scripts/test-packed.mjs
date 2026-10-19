// Packs @octane-xplat/fmt and installs it into a scratch consumer with no
// workspace context, no Rust toolchain, and no repo checkout — proving the
// published artifact loads and formats on its own.
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, readdirSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))

const built = readdirSync(join(root, 'crates/node')).filter((name) =>
	name.endsWith('.node'),
)
if (built.length === 0) {
	console.error('no native artifact under crates/node — run `pnpm build` first')
	process.exit(2)
}
for (const name of built) copyFileSync(join(root, 'crates/node', name), join(root, name))

try {
	const tgz = execFileSync('pnpm', ['pack'], { cwd: root, encoding: 'utf8' })
		.trim()
		.split('\n')
		.at(-1)

	const consumer = mkdtempSync(join(tmpdir(), 'xplat-fmt-consumer-'))
	execFileSync('pnpm', ['init'], { cwd: consumer })
	execFileSync('pnpm', ['add', join(root, tgz)], { cwd: consumer, stdio: 'inherit' })

	const smoke = `
    import { createFormatter } from '@octane-xplat/fmt'
    const fmt = await createFormatter({ cwd: process.cwd() })
    const result = await fmt.formatText({ path: 'demo.tsrx', source: 'export function V() @{ const x = 1; <p>{x}</p> }' })
    if (!result.ok) { console.error(result.diagnostics); process.exit(1) }
    console.log(result.code)
    for (const extension of ['d.ts', 'd.mts', 'd.cts']) {
      const path = 'contracts/repro.' + extension
      const first = await fmt.formatText({ path, source: 'export const value: string' })
      if (!first.ok || first.code !== 'export const value: string\\n') {
        console.error(path, first); process.exit(1)
      }
      const second = await fmt.formatText({ path, source: first.code })
      if (!second.ok || second.changed || second.code !== first.code) {
        console.error(path, second); process.exit(1)
      }
    }
  `
	const out = execFileSync('node', ['--input-type=module', '-e', smoke], {
		cwd: consumer,
		encoding: 'utf8',
	})
	if (!out.includes('<p>')) {
		console.error('packed consumer produced unexpected output:', out)
		process.exit(1)
	}
	console.log('packed consumer formatted TSRX and declaration files in-process: OK')
	rmSync(consumer, { recursive: true, force: true })
} finally {
	for (const name of built) rmSync(join(root, name), { force: true })
}

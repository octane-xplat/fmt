import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { bump, commitBump, gate, nextRelease, notes, validVersion } from './release.mjs'

function repository(t) {
	const root = mkdtempSync(join(tmpdir(), 'fmt-release-'))
	t.after(() => rmSync(root, { recursive: true, force: true }))
	const git = (...args) =>
		execFileSync('git', args, {
			cwd: root,
			encoding: 'utf8',
			stdio: ['ignore', 'pipe', 'pipe'],
		}).trim()
	git('init', '-b', 'main')
	git('config', 'user.name', 'Release Test')
	git('config', 'user.email', 'release@example.com')
	const write = (path, body) => writeFileSync(join(root, path), body)
	const commit = (message, path = 'src/index.js') => {
		write(path, `${Math.random()}\n`)
		git('add', '.')
		git('commit', '-m', message)
		const sha = git('rev-parse', 'HEAD')
		git('update-ref', 'refs/remotes/origin/main', sha)
		return sha
	}
	mkdirSync(join(root, 'src'))
	write(
		'package.json',
		JSON.stringify({
			name: '@octane-xplat/fmt',
			version: '0.1.1',
			optionalDependencies: { '@octane-xplat/fmt-darwin-arm64': '0.1.1' },
		}),
	)
	mkdirSync(join(root, 'npm/darwin-arm64'), { recursive: true })
	write(
		'npm/darwin-arm64/package.json',
		JSON.stringify({ name: '@octane-xplat/fmt-darwin-arm64', version: '0.1.1' }),
	)
	write('Cargo.toml', '[workspace.package]\nversion = "0.1.1"\n')
	write(
		'Cargo.lock',
		'name = "octane-fmt-core"\nversion = "0.1.1"\n\nname = "octane-fmt-node"\nversion = "0.1.1"\n\nname = "other"\nversion = "0.1.1"\n',
	)
	commit('feat: initial formatter')
	git('tag', 'v0.1.1')
	return { root, git, write, commit }
}

test('Conventional Commit policy matches octane-xplat', () => {
	for (const [message, expected] of [
		['feat: new API', 'minor'],
		['fix(core): handle input', 'patch'],
		['perf: faster', 'patch'],
		['refactor: simplify', 'patch'],
		['revert: undo', 'patch'],
		['feat!: replace API', 'breaking'],
		['fix: new behavior\n\nBREAKING CHANGE: migration', 'breaking'],
		['feat(ci)!: internal', undefined],
		['ci: workflow', undefined],
		['chore: dependencies', undefined],
		['docs: release instructions', undefined],
		['unstructured message', undefined],
	]) {
		assert.equal(commitBump(message), expected, message)
	}
})

test('version inference uses reachable tags, package paths, and largest bump', (t) => {
	const { root, git, commit } = repository(t)
	assert.deepEqual(nextRelease(root), { should_release: false })
	commit('feat: documentation example', 'README.md')
	assert.equal(nextRelease(root).should_release, false)
	commit('fix: repair formatting')
	assert.equal(nextRelease(root).version, '0.1.2')
	commit('feat: add syntax')
	assert.equal(nextRelease(root).version, '0.2.0')
	git('tag', 'v0.2.0-nightly.1')
	assert.equal(nextRelease(root).version, '0.2.0')
	assert.equal(nextRelease(root, '0.1.1').should_release, false)
	assert.equal(nextRelease(root, '0.3.0-rc.1').version, '0.3.0-rc.1')
	git('tag', 'v1.0.0')
	commit('feat!: replace API')
	assert.equal(nextRelease(root).version, '2.0.0')
})

test('version validation rejects malformed and injectable overrides', () => {
	for (const value of ['v1.2.3', '1.2', '01.2.3', '1.2.3-01', '1.2.3\npublish=true', '1.2.3 ']) {
		assert.throws(() => validVersion(value))
	}
	assert.equal(validVersion('0.0.0-nightly.g012abcd'), '0.0.0-nightly.g012abcd')
})

test('bump aligns npm, optional dependencies, Rust workspace and local lock entries', (t) => {
	const { root } = repository(t)
	bump(root, '0.0.0-nightly.g012abcd')
	for (const path of ['package.json', 'npm/darwin-arm64/package.json']) {
		assert.equal(JSON.parse(readFileSync(join(root, path))).version, '0.0.0-nightly.g012abcd')
	}
	assert.equal(
		JSON.parse(readFileSync(join(root, 'package.json'))).optionalDependencies[
			'@octane-xplat/fmt-darwin-arm64'
		],
		'0.0.0-nightly.g012abcd',
	)
	assert.match(readFileSync(join(root, 'Cargo.toml'), 'utf8'), /version = "0.0.0-nightly.g012abcd"/)
	const lock = readFileSync(join(root, 'Cargo.lock'), 'utf8')
	assert.equal(lock.match(/version = "0.0.0-nightly.g012abcd"/g).length, 2)
	assert.match(lock, /name = "other"\nversion = "0.1.1"/)
})

test('release notes preview is read-only; preparation consumes notes and preserves history', (t) => {
	const { root, write } = repository(t)
	mkdirSync(join(root, '.changelog/fixes'), { recursive: true })
	write('.changelog/syntax.md', '### More syntax\n\nFormat more source files.\n')
	write('.changelog/fixes/crash.md', '### Handle empty input\n\nEmpty input no longer crashes.\n')
	write('CHANGELOG.md', '# Changelog\n\n## Older release\n\nExisting notes.\n')
	assert.match(notes(root, '0.2.0'), /More syntax/)
	assert.ok(existsSync(join(root, '.changelog/syntax.md')))
	notes(root, '0.2.0', true)
	assert.ok(!existsSync(join(root, '.changelog/syntax.md')))
	assert.ok(!existsSync(join(root, '.changelog/fixes/crash.md')))
	assert.match(readFileSync(join(root, 'CHANGELOG.md'), 'utf8'), /More syntax[\s\S]*Older release/)
	assert.match(readFileSync(join(root, 'FIXLOG.md'), 'utf8'), /Handle empty input/)
})

test('stable gate skips historical tags and completed releases, resumes exact tagged revision', (t) => {
	const { root, git, write, commit } = repository(t)
	const legacy = git('rev-parse', 'HEAD')
	assert.equal(gate(root, 'stable', legacy, { releaseExists: () => false }).publish, false)
	mkdirSync(join(root, 'scripts'))
	write('scripts/release.mjs', '// release automation')
	bump(root, '0.1.2')
	const release = commit('chore(release): v0.1.2')
	assert.equal(gate(root, 'stable', release, { releaseExists: () => false }).sha, release)
	git('tag', 'v0.1.2')
	const newer = commit('fix: another change')
	assert.equal(gate(root, 'stable', newer, { releaseExists: () => false }).sha, release)
	assert.equal(gate(root, 'stable', newer, { releaseExists: () => true }).publish, false)
})

test('nightly gate skips unchanged or newer published revisions and detects package changes', (t) => {
	const { root, git, commit } = repository(t)
	const old = git('rev-parse', 'HEAD')
	const publishedNightly = () => `0.0.0-nightly.g${old.slice(0, 7)}`
	assert.equal(gate(root, 'nightly', old, { publishedNightly }).publish, false)
	const docs = commit('docs: update instructions', 'README.md')
	assert.equal(gate(root, 'nightly', docs, { publishedNightly }).publish, false)
	const next = commit('fix: format syntax')
	const decision = gate(root, 'nightly', next, { publishedNightly })
	assert.equal(decision.publish, true)
	assert.equal(decision.version, `0.0.0-nightly.g${next.slice(0, 7)}`)
	assert.equal(decision.dist_tag, 'nightly')
	assert.equal(
		gate(root, 'nightly', old, { publishedNightly: () => decision.version }).publish,
		false,
	)
})

test('publisher packs all missing packages first and resumes without reuploading engines', (t) => {
	const { root, write } = repository(t)
	const bin = join(root, 'bin')
	mkdirSync(bin)
	write('npm/darwin-arm64/octane-fmt.darwin-arm64.node', 'native test fixture')
	const platformFile = join(root, 'npm/darwin-arm64/package.json')
	const manifest = JSON.parse(readFileSync(platformFile, 'utf8'))
	manifest.main = 'octane-fmt.darwin-arm64.node'
	writeFileSync(platformFile, JSON.stringify(manifest))
	const log = join(root, 'npm-calls.jsonl')
	const fakeNpm = `#!/usr/bin/env node
import { appendFileSync } from 'node:fs'
const args = process.argv.slice(2)
appendFileSync(process.env.TEST_NPM_LOG, JSON.stringify(args) + '\\n')
if (args[0] === 'view') {
  if (process.env.TEST_NPM_FAILURE) { console.error('network error'); process.exit(1) }
  if (process.env.TEST_PLATFORM_EXISTS && args[1].includes('darwin-arm64')) console.log('"0.1.1"')
  else { console.log(JSON.stringify({error: {code: 'E404'}})); process.exit(1) }
} else if (args[0] === 'pack') {
  if (process.env.TEST_PACK_FAILURE && !process.cwd().endsWith('/npm/darwin-arm64')) process.exit(1)
  console.log(JSON.stringify([{filename: 'test.tgz'}]))
}
`
	write('bin/package.json', '{"type":"module"}')
	write('bin/npm', fakeNpm)
	execFileSync('chmod', ['+x', join(bin, 'npm')])
	const script = new URL('./release.mjs', import.meta.url).pathname
	const env = {
		...process.env,
		PATH: `${bin}:${process.env.PATH}`,
		TEST_NPM_LOG: log,
		TEST_ROOT: root,
	}
	const invoke = (extra = {}) =>
		execFileSync(process.execPath, [script, 'publish', 'latest'], {
			cwd: root,
			env: { ...env, ...extra },
			stdio: 'pipe',
		})
	const calls = () => readFileSync(log, 'utf8').trim().split('\n').map(JSON.parse)
	invoke()
	assert.deepEqual(
		calls().map((args) => args[0]),
		['view', 'pack', 'view', 'pack', 'publish', 'publish'],
	)
	assert.match(calls().filter((args) => args[0] === 'publish')[0][1], /npm\/darwin-arm64/)
	writeFileSync(log, '')
	invoke({ TEST_PLATFORM_EXISTS: '1' })
	assert.equal(calls().filter((args) => args[0] === 'publish').length, 1)
	writeFileSync(log, '')
	assert.throws(() => invoke({ TEST_PACK_FAILURE: '1' }))
	assert.equal(calls().filter((args) => args[0] === 'publish').length, 0)
	writeFileSync(log, '')
	assert.throws(() => invoke({ TEST_NPM_FAILURE: '1' }))
	assert.equal(calls().filter((args) => args[0] === 'publish').length, 0)
})

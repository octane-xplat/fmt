import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { existsSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export const releasePaths = [
	'src',
	'crates',
	'npm',
	'package.json',
	'pnpm-lock.yaml',
	'Cargo.toml',
	'Cargo.lock',
	'rust-toolchain.toml',
]

const read = (path) => readFileSync(path, 'utf8')
const json = (path) => JSON.parse(read(path))
const git = (root, ...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim()
const run = (command, args, cwd) => execFileSync(command, args, { cwd, stdio: 'inherit' })

export function validVersion(value) {
	assert.match(
		value,
		/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[\da-zA-Z-]+(?:\.[\da-zA-Z-]+)*)?$/,
		'invalid release version',
	)
	assert.ok(
		!value
			.split('-')
			.slice(1)
			.join('-')
			.split('.')
			.some((part) => /^0\d+$/.test(part)),
		'invalid numeric prerelease identifier',
	)
	return value
}

export function commitBump(message) {
	const header = /^([a-z]+)(?:\(([^\r\n)]+)\))?(!)?:\s+\S/.exec(message)
	if (!header || header[2] === 'ci') {
		return undefined
	}
	if (header[3] || /^BREAKING[ -]CHANGE:\s+\S/m.test(message)) {
		return 'breaking'
	}
	if (header[1] === 'feat') {
		return 'minor'
	}
	if (['fix', 'perf', 'refactor', 'revert'].includes(header[1])) {
		return 'patch'
	}
}

export function nextRelease(root, override = '') {
	const previous = git(root, 'tag', '--merged', 'HEAD', '--list', 'v*', '--sort=-version:refname')
		.split('\n')
		.find((tag) => /^v\d+\.\d+\.\d+$/.test(tag))
	let version = override && validVersion(override)
	if (!version) {
		const bumps = git(
			root,
			'log',
			'--format=%B%x00',
			previous ? `${previous}..HEAD` : 'HEAD',
			'--',
			...releasePaths,
		)
			.split('\0')
			.map((message) => commitBump(message.trim()))

		if (!bumps.some(Boolean)) {
			return { should_release: false }
		}

		const [major, minor, patch] = (previous?.slice(1) ?? '0.0.0').split('.').map(Number)
		version = !previous
			? '0.1.0'
			: bumps.includes('breaking') && major > 0
				? `${major + 1}.0.0`
				: bumps.includes('breaking') || bumps.includes('minor')
					? `${major}.${minor + 1}.0`
					: `${major}.${minor}.${patch + 1}`
	}
	const tag = `v${version}`
	return { should_release: !git(root, 'tag', '--list', tag), version, tag }
}

export function packages(root) {
	return [
		...readdirSync(join(root, 'npm'))
			.sort()
			.map((name) => join(root, 'npm', name)),
		root,
	].map((dir) => ({ dir, manifest: json(join(dir, 'package.json')) }))
}

export function bump(root, version) {
	validVersion(version)
	for (const { dir, manifest } of packages(root)) {
		manifest.version = version
		if (dir === root) {
			for (const name of Object.keys(manifest.optionalDependencies)) {
				manifest.optionalDependencies[name] = version
			}
		}

		writeFileSync(
			join(dir, 'package.json'),
			`${JSON.stringify(manifest, null, dir === root ? 2 : '\t')}\n`,
		)
	}
	const cargo = join(root, 'Cargo.toml')
	writeFileSync(
		cargo,
		read(cargo).replace(/(\[workspace\.package\]\s*\nversion = ")[^"]+/, `$1${version}`),
	)
	const lock = join(root, 'Cargo.lock')
	writeFileSync(
		lock,
		read(lock).replace(/(name = "octane-fmt-(?:core|node)"\nversion = ")[^"]+/g, `$1${version}`),
	)
}

export function notes(root, version, prepare = false) {
	validVersion(version)
	const sections = []
	for (const [folder, log] of [
		['.changelog', 'CHANGELOG.md'],
		['.changelog/fixes', 'FIXLOG.md'],
	]) {
		const path = join(root, folder)
		const files = existsSync(path)
			? readdirSync(path)
					.filter((name) => name.endsWith('.md'))
					.sort()
			: []

		const bodies = files.map((name) => {
			const body = read(join(path, name)).trim()
			assert.match(
				body,
				/^### .+\n\n\S/,
				`${folder}/${name}: expected a level-three title and description`,
			)
			return body
		})

		if (!bodies.length) {
			continue
		}

		sections.push(`## ${log === 'CHANGELOG.md' ? 'Changes' : 'Fixes'}\n\n${bodies.join('\n\n')}`)
		if (prepare) {
			const target = join(root, log)
			const old = existsSync(target)
				? read(target)
				: `# ${log === 'CHANGELOG.md' ? 'Changelog' : 'Fixlog'}\n`

			assert.ok(!old.includes(`## [${version}]`), `${version} already recorded in ${log}`)
			const heading = `## [${version}](https://github.com/octane-xplat/fmt/releases/tag/v${version}) — ${new Date().toISOString().slice(0, 10)}`
			const [title, ...rest] = old.split('\n')
			writeFileSync(target, `${title}\n\n${heading}\n\n${bodies.join('\n\n')}\n${rest.join('\n')}`)
			for (const name of files) {
				rmSync(join(path, name))
			}
		}
	}
	const body = sections.join('\n\n') || `Release @octane-xplat/fmt ${version}.`
	if (prepare) {
		writeFileSync(join(root, '.release-notes.md'), `${body}\n`)
	}
	return body
}

// Only a missing object is absence. Authentication/network errors must stop releases.
export function githubReleaseExists(tag) {
	const result = spawnSync(
		'gh',
		['api', `repos/${process.env.GITHUB_REPOSITORY}/releases/tags/${tag}`],
		{ encoding: 'utf8' },
	)
	if (result.status === 0) {
		return true
	}
	if (/HTTP 404/.test(result.stderr)) {
		return false
	}
	throw new Error(result.stderr || 'GitHub release lookup failed')
}

export function registryVersion(name, version) {
	const result = spawnSync(
		'npm',
		['view', `${name}@${version}`, 'version', '--json', '--registry=https://registry.npmjs.org'],
		{ encoding: 'utf8' },
	)
	if (result.status === 0) {
		return JSON.parse(result.stdout || 'null')
	}
	let error
	try {
		error = JSON.parse(result.stdout).error
	} catch {}
	if (error?.code === 'E404') {
		return undefined
	}
	throw new Error(result.stderr || 'npm version lookup failed')
}

export function publish(root, tag) {
	assert.ok(['latest', 'next'].includes(tag), 'unsupported dist-tag')
	const all = packages(root)
	const version = validVersion(all.at(-1).manifest.version)
	assert.equal(version.includes('-'), tag !== 'latest', 'prereleases must not update latest')
	const tarballs = []
	for (const { dir, manifest } of all) {
		assert.equal(manifest.version, version, `${manifest.name}: version mismatch`)
		if (dir !== root) {
			assert.ok(existsSync(join(dir, manifest.main)), `${manifest.name}: missing native binary`)
		} else {
			for (const platform of all.slice(0, -1)) {
				assert.equal(manifest.optionalDependencies[platform.manifest.name], version)
			}
		}

		if (registryVersion(manifest.name, version)) {
			continue
		}

		const packed = JSON.parse(
			execFileSync('npm', ['pack', '--json', '--ignore-scripts'], { cwd: dir, encoding: 'utf8' }),
		)

		tarballs.push(join(dir, packed[0].filename))
	}
	// Pack and validate every missing package before publishing any; engines go first.
	for (const path of tarballs) {
		run('npm', ['publish', path, '--access', 'public', '--provenance', '--tag', tag], root)
	}
}

export function gate(root, sha, { releaseExists = githubReleaseExists } = {}) {
	assert.match(sha, /^[a-f0-9]{40}$/)
	git(root, 'merge-base', '--is-ancestor', sha, 'origin/main')
	const version = json(join(root, 'package.json')).version
	validVersion(version)
	const tag = `v${version}`
	if (releaseExists(tag)) {
		return { publish: false }
	}

	const tagged = git(root, 'tag', '--list', tag)
	// Tags predating this automation were published manually without GitHub Releases.
	if (
		tagged &&
		spawnSync('git', ['cat-file', '-e', `${tag}:scripts/release.mjs`], { cwd: root }).status !== 0
	) {
		return { publish: false }
	}

	// An existing tag marks a qualified partial publication. Resume that exact revision.
	return {
		publish: true,
		version,
		tag,
		sha: tagged ? git(root, 'rev-parse', `${tag}^{commit}`) : sha,
		dist_tag: version.includes('-') ? 'next' : 'latest',
	}
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
	try {
		const [command, arg] = process.argv.slice(2)
		const root = process.cwd()
		let result
		if (command === 'version') {
			result = nextRelease(root, arg)
		} else if (command === 'bump') {
			bump(root, arg)
		} else if (command === 'prepare') {
			notes(root, arg, true)
		} else if (command === 'preview') {
			console.log(notes(root, arg))
		} else if (command === 'publish') {
			publish(root, arg)
		} else if (command === 'gate') {
			result = gate(root, arg)
		} else {
			throw new Error(
				'usage: release.mjs version [override] | bump|prepare|preview <version> | publish <dist-tag> | gate <sha>',
			)
		}

		if (result) {
			for (const [key, value] of Object.entries(result)) {
				console.log(`${key}=${value}`)
			}
		}
	} catch (error) {
		console.error(error.message)
		process.exitCode = 1
	}
}

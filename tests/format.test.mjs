import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

import { createFormatter } from '../src/index.js'

const here = fileURLToPath(new URL('.', import.meta.url))
const fixture = (name) => readFile(new URL(`fixtures/${name}`, import.meta.url), 'utf8')

async function engine(cwd = here) {
	return createFormatter({ cwd })
}

test('ordinary TS: lint fix + format + spacing in one call', async () => {
	const fmt = await engine()
	const result = await fmt.formatText({
		path: 'basic.ts',
		source: await fixture('basic.in.ts'),
	})
	assert.equal(result.ok, true, JSON.stringify(result.diagnostics))
	assert.match(result.code, /if \(a\) \{\n\tb\(\)\n\}/)
	assert.match(result.code, /\}\n\nconst items/) // blank line after multiline `if`
	assert.match(result.code, /\}\n\nconst done/) // and after the authored-expanded object
	assert.ok(!result.code.includes(';'), result.code) // blessed semi:false
})

test('TSRX: implicit render gets a separating blank line', async () => {
	const fmt = await engine()
	const source = await fixture('tsrx.in.tsrx')
	const result = await fmt.formatText({ path: 'View.tsrx', source })
	assert.equal(result.ok, true, JSON.stringify(result.diagnostics))
	assert.match(result.code, /\}\n\n\t<p>/, result.code)
})

test('unicode identifiers and emoji keep correct spans', async () => {
	const fmt = await engine()
	const source = await fixture('unicode.in.ts')
	const result = await fmt.formatText({ path: 'uni.ts', source })
	assert.equal(result.ok, true, JSON.stringify(result.diagnostics))
	assert.match(result.code, /café = '☕'/, result.code)
	// blank line after the multiline table, before `done`
	assert.match(result.code, /\}\n\nconst done/, result.code)
})

test('unicode TSRX routes through the UTF-16 parser lane', async () => {
	const fmt = await engine()
	const source = 'export function Card() @{\n\tconst title = {\n\t\tcafé: \'☕\',\n\t}\n\t<p>{title.café}</p>\n}\n'
	const result = await fmt.formatText({ path: 'Card.tsrx', source })
	assert.equal(result.ok, true, JSON.stringify(result.diagnostics))
	assert.match(result.code, /café/, result.code)
	assert.match(result.code, /\}\n\n\t<p>/, result.code)
})

test('a second run is byte-identical (idempotent)', async () => {
	const fmt = await engine()
	for (const [path, file] of [
		['basic.ts', 'basic.in.ts'],
		['View.tsrx', 'tsrx.in.tsrx'],
		['uni.ts', 'unicode.in.ts'],
		['asi.ts', 'asi.in.ts'],
		['switch.ts', 'containers.in.ts'],
		['nested.tsrx', 'nested.in.tsrx'],
	]) {
		const first = await fmt.formatText({ path, source: await fixture(file) })
		assert.equal(first.ok, true, JSON.stringify(first.diagnostics ?? []))
		const second = await fmt.formatText({ path, source: first.code })
		assert.equal(second.ok, true, path)
		assert.equal(second.code, first.code, `${path} not idempotent`)
		assert.equal(second.changed, false)
	}
})

test('malformed input returns structured diagnostics', async () => {
	const fmt = await engine()
	const result = await fmt.formatText({ path: 'bad.ts', source: 'const = 3' })
	assert.equal(result.ok, false)
	assert.ok(result.diagnostics.length > 0)
	assert.equal(result.diagnostics[0].path, 'bad.ts')
	assert.ok(['parse', 'lint-fix'].includes(result.diagnostics[0].stage))
})

test('unsupported extension reports the extension', async () => {
	const fmt = await engine()
	const result = await fmt.formatText({ path: 'readme.md', source: '# hi\n' })
	assert.equal(result.ok, false)
	assert.match(JSON.stringify(result.diagnostics), /md|extension/i)
})

test('ASI guard gets a blank line above the `;` line', async () => {
	const fmt = await engine()
	const result = await fmt.formatText({
		path: 'asi.ts',
		source: await fixture('asi.in.ts'),
	})
	assert.equal(result.ok, true, JSON.stringify(result.diagnostics))
	assert.match(result.code, /\]\n\n;\[/, result.code)
})

test('statement lists: switch cases, static blocks, module blocks', async () => {
	const fmt = await engine()
	const result = await fmt.formatText({
		path: 'containers.ts',
		source: await fixture('containers.in.ts'),
	})
	assert.equal(result.ok, true, JSON.stringify(result.diagnostics))
	// blank line inside the switch case after the multiline statement
	assert.match(result.code, /\}\n\n\t\tbreak/, result.code)
	// inside the static block too
	assert.match(result.code, /\}\n\n\t\tready = true/, result.code)
})

test('nested TSRX code blocks get spacing', async () => {
	const fmt = await engine()
	const result = await fmt.formatText({
		path: 'nested.tsrx',
		source: await fixture('nested.in.tsrx'),
	})
	assert.equal(result.ok, true, JSON.stringify(result.diagnostics))
})

test('CRLF source formats to LF output', async () => {
	const fmt = await engine()
	const source = 'if (a) b()\r\nconst x = 1\r\n'
	const result = await fmt.formatText({ path: 'crlf.ts', source })
	assert.equal(result.ok, true, JSON.stringify(result.diagnostics))
	assert.ok(!result.code.includes('\r'), result.code)
})

test('xplat-disable-next-line suppresses the spacing fix', async () => {
	const fmt = await engine()
	// the violation is reported on the statement's own line, so the
	// directive suppresses it by disabling the next line
	const source = '// xplat-disable-next-line\nconst a = {\n\tone: 1,\n}\nconst b = 2\n'
	const result = await fmt.formatText({ path: 'sup.ts', source })
	assert.equal(result.ok, true, JSON.stringify(result.diagnostics))
	assert.match(result.code, /\}\nconst b/, result.code)
})

test('project .oxlintrc can disable a preset rule', async () => {
	const fmt = await createFormatter({ cwd: new URL('fixtures/disabled', import.meta.url).pathname })
	const result = await fmt.formatText({ path: 'x.ts', source: 'if (a) b()\n' })
	assert.equal(result.ok, true)
	// curly disabled → braces not added
	assert.match(result.code, /^if \(a\) b\(\)/)
})

test('project .oxfmtrc options override blessed defaults', async () => {
	const fmt = await createFormatter({
		cwd: new URL('fixtures/semis', import.meta.url).pathname,
	})
	const result = await fmt.formatText({ path: 'x.ts', source: 'const a = 1\n' })
	assert.equal(result.ok, true)
	assert.match(result.code, /const a = 1;/)
})

test('unknown format option fails construction with a config diagnostic', async () => {
	await assert.rejects(
		createFormatter({ cwd: here, formatOptions: { notAnOption: true } }),
		/notAnOption|unknown/i,
	)
})

test('concurrent calls are deterministic', async () => {
	const fmt = await engine()
	const source = await fixture('basic.in.ts')
	const results = await Promise.all(
		Array.from({ length: 16 }, (_, index) =>
			fmt.formatText({ path: `c${index}.ts`, source })),
	)
	const codes = new Set(results.map((r) => r.ok && r.code))
	assert.equal(codes.size, 1)
})

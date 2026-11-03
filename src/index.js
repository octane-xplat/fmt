// @octane-xplat/fmt — Node-API loader and public surface.
// Loads the prebuilt native engine for the host platform, a local build under
// crates/node/ during development, or an explicit OCTANE_FMT_NATIVE path.
// Formatting never spawns oxlint/oxfmt/node/rustc: the engine runs in-process.
import { extname } from 'node:path'
import { format } from 'oxfmt'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'

export { supportsPath } from './support.js'

const require = createRequire(import.meta.url)

const PLATFORM_PACKAGES = {
	'darwin-arm64': '@octane-xplat/fmt-darwin-arm64',
	'darwin-x64': '@octane-xplat/fmt-darwin-x64',
	'linux-arm64-gnu': '@octane-xplat/fmt-linux-arm64-gnu',
	'linux-x64-gnu': '@octane-xplat/fmt-linux-x64-gnu',
	'linux-arm64-musl': '@octane-xplat/fmt-linux-arm64-musl',
	'linux-x64-musl': '@octane-xplat/fmt-linux-x64-musl',
	'win32-arm64': '@octane-xplat/fmt-win32-arm64',
	'win32-x64': '@octane-xplat/fmt-win32-x64',
}

function hostKey() {
	const { platform, arch } = process
	const libc =
		platform === 'linux' && reportMusl() ? 'musl' : 'gnu'
	return platform === 'linux' ? `${platform}-${arch}-${libc}` : `${platform}-${arch}`
}

let musl
function reportMusl() {
	if (musl !== undefined) return musl
	try {
		// Node exposes musl/glibc through process.report on Linux.
		const report = process.report?.getReport?.()
		musl = report?.header?.glibcVersionRuntime === undefined
	} catch {
		musl = false
	}
	return musl
}

function candidates() {
	const key = hostKey()
	const list = []
	if (process.env.OCTANE_FMT_NATIVE) list.push(process.env.OCTANE_FMT_NATIVE)
	list.push(fileURLToPath(new URL(`../octane-fmt.${key}.node`, import.meta.url)))
	list.push(fileURLToPath(new URL(`../crates/node/octane-fmt.${key}.node`, import.meta.url)))
	const pkg = PLATFORM_PACKAGES[key]
	if (pkg) list.push(pkg)
	return { key, list }
}

function loadNative() {
	const { key, list } = candidates()
	const tried = []
	for (const candidate of list) {
		try {
			return require(candidate)
		} catch (error) {
			tried.push(`${candidate}: ${error.code ?? error.message}`)
		}
	}
	throw new Error(
		`@octane-xplat/fmt has no native engine for this host (${key}). ` +
			`Tried: ${tried.join(', ')}. ` +
			`Install the matching platform package or set OCTANE_FMT_NATIVE.`,
	)
}

const native = loadNative()

/**
 * @param {import('./index.d.ts').FormatterOptions} options
 * @returns {Promise<import('./index.d.ts').Formatter>}
 */
export async function createFormatter(options) {
	if (!options || typeof options.cwd !== 'string') {
		throw new TypeError('createFormatter requires { cwd: string }')
	}
	const formatter = native.createFormatter(options)
	return {
		shouldIgnore: (path) => formatter.shouldIgnore(path),
		async formatText({ path, source }) {
			if (/^\.(?:[cm]?[jt]s|[jt]sx|tsrx)$/.test(extname(path))) {
				return formatter.formatText({ path, source })
			}
			try {
				const result = await format(path, source, formatter.formatOptionsFor(path))
				if (result.errors.length) return { ok: false, diagnostics: result.errors.map((error) => ({
					path, stage: 'format', code: 'OXFMT_FORMAT', message: error.message,
					...(error.labels?.[0] ? location(source, error.labels[0].start) : {}),
				})) }
				return { ok: true, code: result.code, changed: result.code !== source }
			} catch (error) {
				return { ok: false, diagnostics: [{ path, stage: 'format', code: 'OXFMT_FORMAT', message: error.message }] }
			}
		},
	}
}

// Oxfmt labels use UTF-8 byte offsets; public diagnostics use authored line/column.
function location(source, offset) {
	const prefix = Buffer.from(source).subarray(0, offset).toString()
	const lines = prefix.split('\n')
	return { line: lines.length, column: [...lines.at(-1)].length + 1 }
}

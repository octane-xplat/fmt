// @octane-xplat/fmt — Node-API loader and public surface.
// Loads the prebuilt native engine for the host platform, a local build under
// crates/node/ during development, or an explicit OCTANE_FMT_NATIVE path.
// Formatting never spawns oxlint/oxfmt/node/rustc: the engine runs in-process.
import { createRequire } from 'node:module'

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
	list.push(new URL(`../octane-fmt.${key}.node`, import.meta.url).pathname)
	list.push(new URL(`../crates/node/octane-fmt.${key}.node`, import.meta.url).pathname)
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
	return native.createFormatter(options)
}

// Inventory from Oxfmt 0.70.0 / oxc 5a6e37e, core/support.rs.
import { createRequire } from 'node:module'
import { basename, extname } from 'node:path'
const inventory = createRequire(import.meta.url)('./support.json')
const extensions = new Set(['js', 'mjs', 'cjs', 'jsx', 'ts', 'mts', 'cts', 'tsx', 'tsrx',
  'toml', 'importmap', 'json5', 'scss', 'less', 'mdx', 'vue', 'svelte', 'mjml',
  ...Object.entries(inventory).filter(([key]) => key.endsWith('_EXTENSIONS')).flatMap(([, values]) => values)])
const filenames = new Set(Object.entries(inventory).filter(([key]) => key.endsWith('_FILENAMES') && key !== 'EXCLUDE_FILENAMES').flatMap(([, values]) => values))
const excluded = new Set(inventory.EXCLUDE_FILENAMES)
/** Whether the pinned Oxfmt engine recognizes this path (plugins may require configuration). */
export function supportsPath(path) {
  const name = basename(path)
  if (excluded.has(name)) return false
  return filenames.has(name) || extensions.has(extname(name).slice(1)) ||
    ['.toml.example', '.json.example', '.tfstate.backup', '.start.frag', '.end.frag'].some((suffix) => name.endsWith(suffix))
}

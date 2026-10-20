# @octane-xplat/fmt

The Octane Xplat formatter: one in-process pipeline that applies selected safe
Oxlint fixes, canonical Oxfmt, and the framework's statement-spacing rule to
JS/TS/JSX/TSX and `.tsrx` source. Everything runs in the host Node process —
no `oxlint`/`oxfmt` binaries, no Node subprocesses, no Rust executables.

```ts
import { createFormatter } from "@octane-xplat/fmt";

const formatter = await createFormatter({ cwd: projectRoot });
const result = await formatter.formatText({ path: "src/Cart.tsrx", source });

if (result.ok) {
	console.log(result.code, result.changed);
} else {
	for (const diagnostic of result.diagnostics) console.error(diagnostic);
}
```

`formatText` never reads or writes the file; `path` exists to select syntax
(TSX vs TSRX) and per-file configuration overrides. File discovery,
stdin/stdout plumbing, and writes belong to the `xplat fmt` command.

## What it does per file

1. Collects *safe* fixes for the rules in the registry below, applies a
   deterministic non-overlapping subset, and recomputes until the lint phase
   settles (bounded at 10 passes).
2. Formats through canonical Oxfmt — for `.tsrx`, via the upstream scan →
   project → format → lift pipeline with its drift-settling passes. Authored
   controls, comments, and embedded CSS payloads are preserved byte-exact;
   embedded `<style>` content is kept raw, not formatted.
3. Reparse and apply the spacing rule: a blank line after each multiline
   statement and before a TSRX code block's implicit render expression, with
   the `;(`/`;[` ASI-guard and attached-comment handling of the original JS
   pass.
4. Repeats 1–3 until the output is stable. On oscillation or bound
   exhaustion the call returns a `convergence` diagnostic and produces no
   output — the original source is never partially written.

The engine therefore guarantees `format(format(source)) === format(source)`,
or it fails.

## Blessed defaults and configuration

Formatting defaults are the Xplat style, then the project's discovered or
explicit `.oxfmtrc.json(c)`, then `formatOptions` (the layer explicit CLI
flags ride on):

```json
{ "semi": false, "useTabs": true, "singleQuote": true }
```

`.oxfmtrc` `overrides` (per-`files`/`excludeFiles` globs) and
`ignorePatterns` are honored. `.editorconfig`, JS/TS config modules, and
`embeddedLanguageFormatting` are refused rather than silently ignored — this
matches the upstream engine's own refusals.

Lint fixes come from one explicit registry — `curly` on by default, `no-var`
supported and off. Oxlint's other default rule groups are disabled, and
JavaScript plugins never run. A project `.oxlintrc.json(c)` can set any
registry rule's severity or options (including per-file `overrides`), and its
`env`, `globals`, `settings`, and `ignorePatterns` pass through. `extends`
and non-registry `rules` entries are not forwarded — `extends` is rejected
loudly; a stray non-registry rule is ignored by policy, since those belong to
`xplat lint`, not formatting.

Spacing respects the `xplat-disable`, `xplat-disable-line`, and
`xplat-disable-next-line` comment forms; Oxlint directives govern the lint
fixes inside the engine.

## Coverage

Formats `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.mts`, `.cts`, `.tsx`, and
`.tsrx`. JSON/JSONC, CSS, Markdown, and YAML are **not** covered by this
package; keep your existing tooling for them.

## Native builds

The engine is a Node-API (napi-rs) addon over the pinned `tsrx-org/oxc`
revision `5487f8f11625dbfa8f62e7f17ab22cfe44d18600`. Published platform
packages are named `@octane-xplat/fmt-<platform>` for macOS arm64/x64, Linux
arm64/x64 (glibc and musl), and Windows arm64/x64. Consumers need no Rust
toolchain and no postinstall download.

Development: `pnpm build` compiles the host binary under `crates/node/`.
Pushing a `v*` tag matching `package.json` runs the release workflow: it
builds all eight targets, publishes the `fmt-<platform>` packages first, then
the main package — consumers can only resolve a release whose engines exist.
`pnpm test` runs the Node suite; `pnpm test:packed` packs and exercises the
artifact in a clean consumer. `cargo fmt --check`, `cargo clippy
-- -D warnings`, and `cargo test` cover the Rust side.

## Performance

Measured 2026-10-16 on macOS arm64 (Apple Silicon), Node 26, rustc 1.98.1:
`xplat fmt --check packages/ui` in the octane-xplat repository — 945
JS/TS/TSRX sources, all three pipeline phases per file — runs in ~1.0s wall
cold and warm (work is CPU-bound in process; there is no cache between runs
yet). Lint-parse + format + spacing run without any child process — the Node
suite asserts a stable child-process count across a batch.

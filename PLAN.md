# Octane Xplat formatter implementation plan

Build `@octane-xplat/fmt` in `octane-xplat/fmt`: a Rust formatting engine that
combines selected safe Oxlint fixes, Oxfmt, and Octane Xplat's custom AST fixes.
The existing `xplat` CLI will load this engine through a Node native binding,
making `xplat fmt` the recommended formatter for Octane Xplat projects.

This is an implementation handoff dated 2026-10-16. Source APIs have been
inspected; no integration has been compiled or behavior verified yet. Complete
the feasibility milestone before treating the proposed design as proven.

## 1. Outcome and scope

The intended project workflow is:

```sh
pnpm exec xplat fmt
pnpm exec xplat fmt src packages/app/src
pnpm exec xplat fmt --check
pnpm exec xplat fmt --stdin-filepath src/Cart.tsrx < src/Cart.tsrx
```

These commands are targets to implement, not commands available in this empty
repository. A successful write leaves the selected files formatted and a
subsequent check succeeds without changes.

### Selected direction

- Keep the implementation in the separate `octane-xplat/fmt` repository.
- Integrate the Rust libraries from `tsrx-org/oxc` programmatically.
- Port the existing custom fixable AST behavior from `packages/lint/` to Rust.
- Inject an explicit, versioned preset of fixable Oxlint rules in code.
- Load the Rust engine into the Node process running `xplat`; transformation
  must not launch `oxlint`, `oxfmt`, another Node process, or a Rust executable.
- Support ordinary JavaScript/TypeScript source and TSRX through the same
  public interface. Threads within the process are allowed.

Single process does not mean single parse. Upstream linting, formatting, and
custom passes may need separate parses; optimize only after measuring them.

### Working defaults

The discussion did not settle every policy. Use these defaults during
implementation, keep them visible in documentation, and record a change if
evidence requires different behavior:

| Concern                        | Default                                                                    |
| ------------------------------ | -------------------------------------------------------------------------- |
| Mode                           | Write by default; `--check` computes the same output without writing       |
| Fix safety                     | Only fixes classified as safe; no suggestions or unsafe fixes              |
| Configuration                  | Blessed defaults, then project overrides, then explicit CLI options        |
| Rule overrides                 | Projects can disable a preset rule or change its supported options         |
| Diagnostics                    | Formatting errors fail; unrelated non-fixable lint findings belong to lint |
| Compatibility                  | Keep `xplat-lint` working while adopting the formatter                     |
| First milestone                | JS/TS/JSX/TSX and TSRX source formatting                                   |
| Standalone Rust CLI, LSP, WASM | Deferred; the required delivery is the Node package and `xplat fmt`        |

Do not port all of `packages/lint/src/xplat-rules.mjs` as part of this project.
Those checks primarily report architecture and correctness problems; they are
not currently automatic formatting fixes.

## 2. Evidence and upstream boundaries

Start from `tsrx-org/oxc` revision
`96b581c8d334e0c8636aac8a947536c5f82c8684`, resolved on 2026-10-16. Pin the
chosen revision in Cargo dependencies and commit `Cargo.lock`. If adopting a
different revision, rerun the feasibility checks and update the recorded pin.

| Source                                                                                                                                       | What implementers should inspect                                      |
| -------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| [Toolchain README](https://github.com/tsrx-org/oxc/blob/96b581c8d334e0c8636aac8a947536c5f82c8684/packages/toolchain/README.md)               | npm entry points and the current public toolchain                     |
| [Rust architecture](https://github.com/tsrx-org/oxc/blob/96b581c8d334e0c8636aac8a947536c5f82c8684/docs/architecture/rust-oxc-core.md)        | Projection, authored-source mapping, and native capability boundaries |
| [Adapter manifest](https://github.com/tsrx-org/oxc/blob/96b581c8d334e0c8636aac8a947536c5f82c8684/crates/oxc_adapter/Cargo.toml)              | Shared OXC revision and feature flags                                 |
| [Lint session](https://github.com/tsrx-org/oxc/blob/96b581c8d334e0c8636aac8a947536c5f82c8684/crates/tsrx_lint/src/session.rs)                | In-memory configuration and safe editor edits                         |
| [Format library](https://github.com/tsrx-org/oxc/blob/96b581c8d334e0c8636aac8a947536c5f82c8684/crates/tsrx_format/src/lib.rs)                | Text formatting, configuration, and settling behavior                 |
| [Parser result](https://github.com/tsrx-org/oxc/blob/96b581c8d334e0c8636aac8a947536c5f82c8684/crates/tsrx_parser_engine/src/parse_result.rs) | The `FlatTape` syntax representation, comments, and completeness      |
| [Parser entry points](https://github.com/tsrx-org/oxc/blob/96b581c8d334e0c8636aac8a947536c5f82c8684/crates/tsrx_parser_engine/src/entry.rs)  | ASCII and UTF-16 routes and parse recovery boundaries                 |

Important constraints to preserve:

- The npm lint entry point currently supplies configuration helpers, not an
  in-process lint runner. Calling the npm formatter for TSRX launches a native
  program. Import the Rust libraries through our own binding instead.
- `LintSession::new_with_config_source` accepts an in-memory JSON rule config.
  A fix-enabled session's `code_actions` returns validated edits without
  writing. `lint_text` rejects fix-enabled sessions; `lint_file` may write.
  Do not compose the formatter around disk-writing upstream calls.
- `FormatSession::format_text` formats caller-owned text. Its current configured
  constructor loads JSON/JSONC from paths. Prove how to inject format defaults
  without temporary configuration files; propose a small upstream constructor
  if necessary rather than duplicating their format pipeline.
- The adapter pins all OXC crates to one Git revision. Reuse that dependency
  graph. Mixing unrelated registry and Git copies can produce incompatible
  allocator and AST types. Disable the adapter's editor feature when unneeded.
- TSRX's reconstructed program is a compact `FlatTape`, not a standard typed
  OXC AST. Build a small traversal adapter over public APIs or obtain an
  appropriate upstream extension. Do not introduce another TSRX parser.
- The plain `parse_tsrx` route currently rejects non-ASCII input. Inspect the
  UTF-16 route and its coordinate domain before applying edits to Rust UTF-8
  strings. Unicode input must work; ASCII-only formatting is not acceptable.
- Type-aware linting uses a separate TypeScript-Go process. Keep it out of this
  formatter. JavaScript lint plugins also remain outside this native pipeline.

### Source behavior to migrate

The inspected framework checkout was at
`8295753813e525d6d8ae2fa848e75ab225899abe`. These links pin that baseline.
Compare with current framework sources before porting so later fixes are
included deliberately:

- [Spacing implementation](https://github.com/octane-xplat/octane-xplat/blob/8295753813e525d6d8ae2fa848e75ab225899abe/packages/lint/src/spacing-rules.mjs)
- [Oxlint plugin adapter](https://github.com/octane-xplat/octane-xplat/blob/8295753813e525d6d8ae2fa848e75ab225899abe/packages/lint/src/oxlint-plugin.mjs)
- [TSRX companion pass](https://github.com/octane-xplat/octane-xplat/blob/8295753813e525d6d8ae2fa848e75ab225899abe/packages/lint/src/lint-tsrx.mjs)
- [Architecture diagnostics](https://github.com/octane-xplat/octane-xplat/blob/8295753813e525d6d8ae2fa848e75ab225899abe/packages/lint/src/xplat-rules.mjs)
- [Lint configuration](https://github.com/octane-xplat/octane-xplat/blob/8295753813e525d6d8ae2fa848e75ab225899abe/.oxlintrc.json)
- [Format configuration](https://github.com/octane-xplat/octane-xplat/blob/8295753813e525d6d8ae2fa848e75ab225899abe/.oxfmtrc.json)
- [Discovery regression tests](https://github.com/octane-xplat/octane-xplat/blob/8295753813e525d6d8ae2fa848e75ab225899abe/scripts/lint-tsrx.test.mjs)

The current fixable custom behavior adds blank lines after multiline
statements and before a TSRX code block's implicit render expression. It also
handles leading semicolons that protect automatic semicolon insertion and
keeps adjacent line comments attached to that guarded statement. Copy the
behavior into independent input/output fixtures before replacing the JS pass.

## 3. Architecture and public boundary

Use a small Cargo workspace and one npm package. This is a suggested layout;
adjust it to the selected binding tool without creating extra public layers:

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
crates/core/                 # configurations, transformations, diagnostics
crates/node/                 # Node-API binding over the core
src/                        # npm loader and TypeScript public surface
tests/fixtures/             # shared source inputs and expected outputs
tests/                      # Node, packed-consumer, and process checks
package.json                # @octane-xplat/fmt
pnpm-lock.yaml
.github/workflows/          # checks and native build matrix
```

Prefer an established Node-API binding tool such as `napi-rs`. The Rust core
must be usable without Node and must not perform source-file writes. Keep the
binding thin: load the correct native module, translate inputs/results, and
reuse configured sessions across files. Build scripts may use subprocesses;
formatting calls may not.

The intended Node interface is a reusable formatter over caller-owned text.
This declaration describes the proposed contract; it is not an existing API:

```ts
export interface FormatterOptions {
  cwd: string;
  lintConfigPath?: string;
  formatConfigPath?: string;
}

export interface FormatDiagnostic {
  path: string;
  stage:
    "config" | "parse" | "lint-fix" | "format" | "custom-fix" | "convergence";
  code: string;
  message: string;
  line?: number;
  column?: number;
}

export type FormatResult =
  | { ok: true; code: string; changed: boolean }
  | { ok: false; diagnostics: FormatDiagnostic[] };

export interface Formatter {
  formatText(input: { path: string; source: string }): Promise<FormatResult>;
}

export declare function createFormatter(
  options: FormatterOptions,
): Promise<Formatter>;
```

`changed` compares final output with the original input, and success returns
the full final source. Errors return no publishable partial output. Use stable
diagnostic codes and documented one-based line/column coordinates. Invalid
configuration should fail construction with a documented structured error.

The `xplat` command owns file discovery, stdin/stdout behavior, check/write
mode, and filesystem updates. It calls the same text API used by consumers.
Do not build separate formatting implementations for the CLI and binding.

## 4. Transformation and configuration contract

For each file, operate on owned buffers in this order:

1. Parse and collect selected safe Oxlint edits against the current source.
2. Apply a deterministic non-overlapping edit set, using authored coordinates.
   Recompute until the lint-fix phase settles, with a bounded pass count.
3. Format the result through the upstream Rust formatter.
4. Parse the formatted source and apply the custom spacing edits. This order
   lets line wrapping determine whether statements count as multiline.
5. Reparse and check the complete pipeline for a stable result. Use a bounded
   limit and detect repeated outputs. On oscillation or exhaustion, return a
   convergence error and leave the original file untouched.

Never reuse spans after changing the buffer they describe. Apply edits from
right to left after checking bounds, overlap, and character boundaries.
Preserve authored TSRX controls and comments; never emit projected glue.

The required invariant is:

```text
format(format(source)) == format(source)
```

Do not blindly require AST equality across the entire pipeline: a selected
lint fix intentionally changes syntax. Verify approved lint transformations
against their expected output and verify semantic structure preservation for
the formatting and whitespace-only passes separately.

### Blessed preset

Keep one explicit rule registry shared by configuration and tests. Include
each rule's options, default state, safe-fix evidence, and TSRX fixture.
Disable Oxlint's other default rule groups explicitly; verify that an unrelated
built-in rule neither transforms input nor fails formatting. Do not enable
every rule tagged fixable automatically.

The existing framework config explicitly enables `curly` and `xplat/spacing`.
Use them as initial candidates, verify that the pinned engine offers the
required safe behavior, and document the initial chosen preset. The `no-var`
rule is a useful upstream-supported feasibility fixture; its use in the spike
does not automatically make it part of the blessed preset.

Read project overrides only for the formatter's supported rule registry;
broader lint rules remain the lint command's responsibility. Do not forward
the framework's entire `.oxlintrc.json` unchanged: it contains JavaScript
plugins and non-fixable `xplat/*` diagnostics. Ignore unrelated lint settings
only according to this documented selection policy. If a requested formatter
capability cannot be honored, report it explicitly.

Keep supported rule options, per-file overrides, relative config paths, and
disable comments consistent across ordinary source and TSRX. Establish
compatibility fixtures for the current `xplat-disable`, `-line`, and
`-next-line` forms and Oxlint directives; investigate inconsistencies in the
old passes rather than preserving accidental bugs silently.

Use programmatic defaults without creating project config files. Reconcile
applicable globals/environments and path-relative configuration when building
the in-memory config. Prove `extends` and overrides before claiming support;
reject unsupported configuration clearly. JS/TS executable config modules and
arbitrary JavaScript plugins are outside the initial formatter contract.

Start the formatting defaults from the framework's current style, then prove
their behavior through the native API:

```json
{
  "semi": false,
  "useTabs": true,
  "singleQuote": true
}
```

Keep framework-specific generated-file exclusions in the integration layer;
do not hard-code monorepo paths into every consumer project's preset.

## 5. CLI and filesystem contract

| Input or mode                     | Required behavior                                                            |
| --------------------------------- | ---------------------------------------------------------------------------- |
| No paths                          | Discover supported source beneath the current directory                      |
| Explicit files/directories        | Restrict discovery to those inputs; deduplicate and sort                     |
| `--check`                         | Full transformation, no writes; report files whose output differs            |
| `--stdin-filepath PATH`           | Read stdin; use the path for syntax/config; write transformed text to stdout |
| Stdin with `--check`              | Emit no transformed source; return whether it would change                   |
| Unsupported explicit file         | Actionable error naming the extension                                        |
| Unsupported file during discovery | Skip according to the documented coverage inventory                          |
| Parse/config/transform error      | Nonzero exit, path and useful diagnostic, original source retained           |

Use exit `0` for success, `1` when check mode finds changes, and `2` for usage,
configuration, parsing, transformation, or I/O failure. Errors take precedence
over check differences. Human diagnostics go to stderr so stdin output stays
usable by editors and pipes. `--check` and write behavior must use the same
configuration and transformation pipeline.

Honor `.gitignore`, explicit formatter ignores, and the project's generated
file policy. Always exclude dependency/build/VCS directories. Preserve the
existing boundary rule for nested repositories, including both `.git`
directories and `.git` files used by worktrees/submodules. Do not follow
symlinks implicitly or traverse outside a discovered root. Report an unmatched
explicit path instead of silently treating it as success.

Transform and validate the selected batch before beginning writes. A
transformation error must result in zero source-file writes. Write changed
files atomically per file, preserve permissions, and detect if the original
changed after it was read. Report I/O failures and any already-written files;
do not claim a whole batch is crash-atomic. Check mode must leave source files
and their modification times unchanged.

### Format coverage before default rollout

The existing native adapter is centered on JS/TS source families and TSRX;
it does not expose the full Oxfmt CLI's language set. Embedded TSRX CSS is
currently preserved rather than formatted. Build a concrete inventory for
JSON/JSONC, CSS, Markdown, YAML, and other formats present in Xplat projects.

For each format, record the available in-process Rust entry point, options,
fixtures, and any gap. Prefer a small upstream API addition where practical.
Do not silently introduce a subprocess fallback or market source-only
coverage as a replacement for all project formatting. The source formatter
can ship as an explicit milestone, but switching project defaults must carry
documented coverage and retained tooling for any unsupported formats.

## 6. Implementation milestones

### A. Prove the native composition

Create the smallest compilable Rust core and Node binding. Use the pinned
upstream libraries directly, including the configured in-memory lint path.
First prove a single file through safe lint fixing, Oxfmt, and one spacing
fix, then repeat with TSRX and Unicode source.

Acceptance:

- Ordinary TS and TSRX fixtures pass through all three stages in one Node
  process, without launching a formatting child process.
- A rule supplied only through the programmatic preset actually fixes input.
- Format options can be injected without project or temporary config files.
- The custom pass can inspect authored statement boundaries, comments, and
  TSRX implicit render boundaries through public Rust APIs.
- Non-ASCII identifiers, emoji, and text before edit locations retain correct
  spans; malformed input returns a structured failure without writing.
- A second complete run is byte-identical to the first output.
- Record actual compile commands, adopted revisions, and results. If an API
  is missing, identify the smallest upstream change; do not replace this goal
  with command orchestration or a new parser.

### B. Port the custom fixes and implement the core contract

Create input/output fixtures from the existing spacing implementation and
port its behavior. Implement preset merging, safe edit application, complete
pipeline convergence, and useful errors. Reuse upstream projection and
validation rather than recreating TSRX translation.

Acceptance:

- Cover program/block/static block/switch case/TS module statement lists,
  nested TSRX code blocks, and the setup-to-implicit-render boundary.
- Cover existing blank lines, trailing/attached comments, ASI guards before
  `(` and `[`, and statements Oxfmt expands onto multiple lines.
- Cover LF/CRLF, Unicode, directive suppressions, malformed syntax,
  overlapping fixes, disabled rules, and per-file configuration overrides.
- Configuration loads are reused per scope; file order does not change output.
- The Rust result and Node result agree for the same fixture corpus.

### C. Package the binding and prove installation

Publishable artifact preparation belongs here; actual npm publication is a
separate release action. Use prebuilt platform packages selected by the npm
loader, with no Rust compiler or postinstall download required for consumers.
Keep versions of the loader and native artifacts synchronized and provide an
actionable unsupported/missing-platform error.

Target the host matrix used by the upstream toolchain: macOS arm64/x64,
Linux arm64/x64 with glibc and musl, and Windows arm64/x64. The formatter runs
on development/CI hosts; iOS and Android apps do not load this binding.

Acceptance:

- CI builds and packages the native targets; record which run tests on actual
  compatible hosts and which are build-only evidence.
- Install a packed npm artifact in an isolated consumer outside both repos.
  It loads the packaged native library and runs the fixture corpus without
  workspace resolution, a Rust compiler, or invoking external tools.
- Type declarations, export map, Node engine range, native artifact loading,
  MIT attribution, and packaged licenses are checked.
- Test concurrent formatter calls for deterministic results and session safety.

### D. Integrate `xplat fmt` in the framework repository

This milestone changes `octane-xplat/octane-xplat` separately. Read its current
`AGENTS.md` and toolchain, source-context, testing, documentation, recipes, and
release guidance before editing. Preserve unrelated work and commit each
repository's changes independently.

Add the declared `@octane-xplat/fmt` dependency to `packages/cli`, register the
command, and implement discovery and check/write/stdin modes around the shared
text API. Exercise the packed package first; do not depend on an unpublished
workspace-only resolution arrangement.

Acceptance:

- `xplat fmt --help`, path selection, stdin, write, and check modes work in a
  freshly created app and an installed consumer.
- Nested repos, dependencies, generated output, and ignored files remain
  untouched. Running from a nested directory has documented config/root behavior.
- Existing `xplat-lint` and non-fixable architecture diagnostics still work.
- Prevent competing spacing implementations: once Rust parity is proven,
  remove spacing enforcement from the default lint flow or route compatibility
  behavior through the shared engine. Do not leave defaults that fight each other.
- Update supported templates, scripts, CLI docs, agent guidance, and relevant
  recipes together. Record recipe coverage separately from test evidence in
  the framework's Silo database and run `pnpm check:recipes`.
- Run focused tests and the owning repo's required dependent-inclusive
  typechecks for every touched package, using actual package names.

### E. Validate adoption and document release behavior

Run the completed formatter on a representative, disposable checkout of the
framework and on a fresh app. Review the resulting diff for syntax, comments,
TSRX controls, embedded text, and unintended changes. Keep any bulk formatting
commit separate from implementation changes.

Record the chosen preset, supported formats/options, remaining gaps,
upgrade process, and the commands consumers use. Define native artifact and
npm release ordering before publishing. The separate formatter repository
owns its version; the framework owns its dependency update and integration.

## 7. Verification and completion criteria

Add these repository-level commands as the implementation takes shape; they
are planned interfaces for agents and CI, not scripts present today:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm build
pnpm typecheck
pnpm test
pnpm test:packed
```

Use the pinned upstream compiler requirement when choosing `rust-toolchain.toml`
and the supported Node range when building artifacts. Use pnpm for all Node
package operations. Keep fixtures small and aimed at behavior, not copies of
implementation details.

Required evidence includes stable expected outputs, parser/structure checks,
configuration precedence, convergence failure cases, no writes on transform
failure, no writes in check/stdin modes, and isolated installed-package tests.
Verify the process constraint with execution tracing or equivalent process
creation instrumentation over a controlled formatting call; mocking Node's
`child_process` alone cannot detect a Rust dependency spawning a child.

Measure representative warm and cold batches and record workload, host,
versions, elapsed time, and parse/pass counts where available. Establish a
baseline before choosing regression budgets. Do not claim a speedup or
single-parse operation from architecture alone.

The engine is ready when the Rust and installed Node paths meet the same
behavioral contract, the documented host matrix has explicit evidence, and
unsupported capabilities fail clearly. The overall project is complete only
after `xplat fmt` is integrated and the project-default rollout accurately
documents format coverage. An upstream API gap is a concrete dependency to
resolve, not permission to weaken the single-process or source-fidelity goals.

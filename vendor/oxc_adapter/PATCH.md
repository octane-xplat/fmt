# Local declaration-file patch

Source: https://github.com/tsrx-org/oxc/tree/96b581c8d334e0c8636aac8a947536c5f82c8684/crates/oxc_adapter

The source and tests are copied from that revision under its MIT license.
`Cargo.toml` makes inherited package fields explicit, points the tape-schema
dependency at the same Git revision, and removes workspace lint inheritance.

The only implementation change is in `src/lib.rs`: `SourceKind::from_path`
uses OXC’s filename classifier to recognize `.d.ts`, `.d.mts`, and `.d.cts`
and selects a new `TypeScriptDefinition` kind, mapped to `SourceType::d_ts()`.
This preserves ambient mode in lint parsing, safe-fix validation, formatting,
and expanded-object drift reparses. The original adapter erased this mode by
mapping every TypeScript filename to `SourceType::ts()`.

The root Cargo patch applies to both direct and transitive adapter users.
Remove this copy and the Cargo patch when a pinned upstream revision includes
the fix. Public API regressions live in `tests/format.test.mjs`.

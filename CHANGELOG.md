# Changelog

## [0.3.0](https://github.com/octane-xplat/fmt/releases/tag/v0.3.0) — 2026-11-06

### Blank lines around declaration clusters

The spacing pass now separates runs of declaration-kind statements —
directives, `import` statements, `export` statements of every form, and
non-exported variable/function/class and TS type/interface/enum/module
declarations — from surrounding statements with a blank line, even when
every member is single-line. Each of those is its own cluster kind, so
`export` declarations separate from internal ones. The pass also applies
inside function bodies now, matching the JS rule's coverage.

User-facing changes are assembled here by the release PR.

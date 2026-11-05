# Releases

Releases follow the same model as `octane-xplat/octane-xplat`: a maintained
draft release PR, Conventional Commit version inference, stable publication
from green main CI. This repository keeps pnpm, its eight native build targets,
and one shared npm/Rust version.

## Cut a stable release

1. Merge normal changes to `main`. `release-pr.yml` creates or refreshes the
   draft `release/lockstep` PR with versions and release notes. Do not bump
   manifests or push release tags by hand.
2. Mark the draft ready, review the version and CI results, then merge.
3. Once main's `ci` workflow succeeds, `release.yml` builds the tested revision
   for all eight targets, tags it, publishes platform packages followed by
   `@octane-xplat/fmt`, and creates a GitHub Release. That GitHub Release marks
   publication complete. The workflow then refreshes the next release PR.

The next version comes from commits touching `src/`, `crates/`, `npm/`, root
package/Cargo manifests and lockfiles, or `rust-toolchain.toml` since the latest
reachable stable `vX.Y.Z` tag. The largest applicable bump wins:

| Commit | Effect |
| --- | --- |
| `feat:` | Minor |
| `fix:`, `perf:`, `refactor:`, `revert:` | Patch |
| `!` or `BREAKING CHANGE:` / `BREAKING-CHANGE:` footer | Minor below 1.0; major afterward |
| Other types, non-conventional messages, or any `(ci)` scope | No automatic bump |

Inspect the inferred version without editing files:

```sh
pnpm release:version
```

A manual override belongs to the release PR workflow. Blank input preserves
an existing override; `auto` restores inference. Prerelease overrides publish
under npm's `next` tag and create a GitHub prerelease.

```sh
gh workflow run release-pr.yml -f version=0.2.0
gh workflow run release-pr.yml -f version=auto
```

## Release notes

Add `.changelog/<description>.md` for useful features or breaking changes, or
`.changelog/fixes/<description>.md` for bugs shipped in a stable release.
Internal changes and bugs fixed before release need no note. A note starts
with a level-three title, a blank line, and a short description. Include a
minimal usage or migration example when an API changes.

```md
### Format empty documents

Empty documents now format successfully instead of returning an error.
```

Preview notes with `pnpm release:preview 0.2.0`. The release PR moves pending
notes into `CHANGELOG.md` and `FIXLOG.md`, preserving published history.

## Retry a release

For a failed publication, use **Re-run failed jobs**. Missing
packages are packed before any upload and exact versions already on npm are
skipped. A subsequent green main CI run also resumes an incomplete stable
release from its original tag, even if main advanced. Manual stable dispatch
requires current main to have passed CI:

```sh
gh workflow run release.yml
```

Historical tags created before this automation are treated as completed legacy
releases even though they have no GitHub Releases. They are never republished.
Native cross-builds run on GitHub; only the host target is exercised by the
existing Node and packed-consumer CI tests.

## Repository setup

Configure these before enabling the workflows:

1. Add the repository secret `RELEASE_TOKEN`: a fine-grained PAT for this
   repository with Contents, Pull requests, and Workflows read/write. It
   maintains the release PR and pushes tags. A PAT is required so release PR
   writes trigger CI, and tags can include workflow changes.
2. Configure npm trusted publishing for **all nine packages**, using GitHub
   owner `octane-xplat`, repository `fmt`, workflow `release.yml`, with direct
   publishing allowed. This is separate from `RELEASE_TOKEN`; no npm token
   is used. Confirm each package's publishing settings on npmjs.com. Packages
   must already exist before configuring a trusted publisher.
3. Ensure branch rules require the normal `ci` checks on release PRs.

The publisher uses npm on Node 24 with `id-token: write` and provenance.
[npm's trusted publishing requirements](https://docs.npmjs.com/trusted-publishers/)
require npm 11.5.1+ and Node 22.14+; pnpm remains the install/build tool.

Run `pnpm test:release` and `actionlint` when changing release tooling.

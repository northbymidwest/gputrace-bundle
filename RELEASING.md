# Releasing

`gputrace-bundle` is a single pure-Rust crate, released by
`.github/workflows/release.yml`. Every release is a `0.x` pre-release. The whole
flow runs on hosted runners.

## Prerequisites (one-time)

- A crates.io trusted-publisher entry: owner `northbymidwest`, repo
  `gputrace-bundle`, workflow `release.yml`, environment `release`.
- A `release` environment in repo settings with a required reviewer, restricted
  to `main`.

## By hand, before dispatching

1. Bump `version` in `Cargo.toml`.
2. Remove the `publish = false` line (the safety net that keeps the crate off
   crates.io until you mean it).
3. Retitle the `## Unreleased` section of `CHANGELOG.md` to
   `## <version> - <YYYY-MM-DD>`.
4. Commit, push, and wait for CI to go green.

Note: `gputools-replay-hl` depends on this crate. Publish `gputrace-bundle`
before releasing the framework stack, and update hl's dependency there from the
`../gputrace-bundle` path form to a plain version dependency.

## Dispatch

Actions -> release -> Run workflow. Version without a leading `v`. Leave
`dry_run` ticked to rehearse; untick to publish.

- `preflight` validates the version, the manifest, a non-empty `CHANGELOG.md`
  section, and that the newest CI run for the commit is green.
- `publish` pauses at the `release` environment's reviewer gate, then exchanges
  an OIDC token for a short-lived crates.io token, publishes, tags `v<version>`,
  and creates a `--prerelease` GitHub Release from the changelog section.

`dry_run` defaults to true: forgetting to untick costs a re-run; forgetting to
tick would be an irreversible publish.

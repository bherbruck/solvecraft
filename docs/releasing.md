# Releasing SolveCraft

Releases are deliberate operations that an agent can complete end to end. Development lands
on `main`; pushing the selected commit to `release` builds all packages and creates a **draft**
GitHub Release. Publishing that verified draft is a separate operation. Pushing to `main` or
pushing a tag does not start the release workflow. There is no automatic commit-message-based
version bump and no patch release for every merge.

## Choose and prepare the version

1. Fetch `origin` and its tags. Inspect the previous published release and the changes since
   it. Choose one version for the complete batch, based on user-visible behavior:
   - Patch: compatible fixes, reliability improvements, and packaging corrections.
   - Minor: new compatible capabilities. While on `0.x`, also use a minor bump for breaking
     changes and explain the break and any migration in the notes.
   - Major: incompatible public behavior or file-format/API changes after `1.0`. Moving to
     `1.0` is an explicit product decision, not something inferred from a commit prefix.
   - Use `-rc.1`, `-rc.2`, etc. when a release candidate is requested.
   Documentation-only changes normally wait for the next release. State the reason for the
   chosen bump in the release notes. Follow any version or release scope specified by the owner.
2. Update `[workspace.package] version` in root `Cargo.toml`. Then run
   `cargo metadata --format-version 1 --no-deps > /dev/null` to refresh workspace package
   versions in `Cargo.lock`. Inspect the diff and keep unrelated dependency changes out.
   SolveCraft does not currently implement the siblings' `cargo xtask version set` command.
3. Run `cargo xtask ci` and the packaging checks in `.github/workflows/packaging-lint.yml`.
   Commit the version files through the normal review flow and land them on `main`.
   Do not use `[skip ci]`. Record the exact commit SHA to release. If it changes, revalidate it.

## Build the draft

Use an authenticated maintainer account or GitHub App with the repository's normal release
permissions. Follow branch rules; never force-push or bypass required checks.

```sh
git fetch origin --tags
git switch release
git merge --ff-only origin/release
git merge --ff-only origin/main
git push origin release
```

For the first release, after this workflow has landed on `main`, create `release` from the
validated `origin/main` commit (`git switch -c release origin/main`) and push it. If the branch
already exists, use the commands above. Do not create the branch from unrelated work. If a
fast-forward fails, inspect and resolve the divergence through the normal review flow.

Find the `release.yml` Actions run for the recorded SHA and `release` branch, and monitor that
specific run. The workflow builds Linux (both architectures, including Flatpak), macOS,
Windows, FreeBSD, and web packages. Once every required job succeeds it creates a draft
`SolveCraft v<version>` with the artifacts, `SHA256SUMS.txt`, install notes, and generated changes.

A second push with the same untagged version can rebuild and replace that draft's complete
asset set. Existing release notes are preserved on rebuild; refresh their changes section if
new commits were added. An existing tag or published release is never overwritten. Use a new
version after publication. Do not push the version tag yourself: publishing the draft creates
it at the draft's target commit.

## Verify and publish

When the task authorizes shipping a release, agents should complete these steps without
requiring the owner to click through the GitHub UI. A request only to prepare a release or
change release infrastructure does not itself request publication of the application.

1. Confirm that the workflow run succeeded for the intended SHA and that the matching release
   is still a draft targeting that SHA. Avoid publishing while another release build is active.
2. Download all assets to a fresh directory. Run `sha256sum --check SHA256SUMS.txt` and check
   that every expected platform/architecture is present. Smoke-test the built CLI's `--version`
   and an example model on an available platform. Inspect the package-check job output for
   other platforms; report any platform you could not exercise yourself.
3. Review the generated notes. Add a concise description of features, fixes, breaking changes,
   and known limitations. Keep the installation instructions. Packages are currently unsigned;
   do not claim otherwise. Resolve failed checks or missing artifacts before publication.
4. Publish the verified draft using `gh` or the equivalent GitHub release API. For example:

   ```sh
   # Substitute the version that was actually built and verified.
   gh release edit v0.2.0 --draft=false --latest
   # For a release candidate instead:
   gh release edit v0.2.0-rc.1 --draft=false --prerelease --latest=false
   ```

5. Confirm that the release is public and its tag resolves to the recorded SHA. Report the
   release URL, version, checks performed, and any remaining limitations.

## Dry runs and retries

`gh workflow run release.yml --ref <branch> -f draft=false` builds packages and uploads the
`release-assets` Actions artifact without creating or publishing a release. For an explicit
retry that creates/updates a draft, use `--ref release -f draft=true`. Draft creation from
another branch is rejected. A rerun must still use an unpublished, untagged version.

## Flatpak repository coordination

The Flatpak repository proposed in PR #39 consumes **published** releases, not these drafts.
Keep that deployment separate from building the application. A publication made with an
Actions `GITHUB_TOKEN` does not trigger downstream `release: published` workflows; in that
case explicitly dispatch the Flatpak workflow with the published tag, or use an appropriately
authorized maintainer/App token for publication. Do not assume the repository updated: check
the deployment run. Coordinate its GitHub Pages output with the existing docs website.

## Repository setup

The `release` branch is advanced only for intentional release batches. Restrict its writers to
maintainers/release agents through repository rules if required by the owner's policy. The
workflow needs `contents: write` only in its draft-creation job. It does not push commits,
calculate versions, or require a semantic-release bot to bypass `main` protections.

This workflow replaces the every-push-to-main semantic-release proposal in PR #38; do not
merge that automation alongside this release model without redesigning its triggers.

---
name: version-release
description: >-
  Use this skill whenever the user asks to bump, prepare, publish, tag, or
  release a codex-ops version. It selects between two guarded workflows: bump
  the release version on a temporary branch from origin/master and push the
  version branch, create the bump pull request, and optionally wait for checks
  and merge it, or create and push the Git release tag for an already-bumped
  version on master. Keep version selection in the bump workflow and keep the
  tag workflow tag-only.
---

# Version Release

Use this project-specific release workflow for `codex-ops`.

When executing commands in this repository, follow the project shell rule and
run them through `rtk`, for example `rtk git status --short`.

## Choose Workflow

- If the user asks to bump, upgrade, increment, or prepare the release version,
  use the Bump Version Workflow.
- If the user asks to create, add, publish, or push a Git release tag, use the
  Add Tag Workflow.
- If the user asks for a complete release, first run the Bump Version Workflow,
  including its PR creation step. Merge the PR only when the user explicitly
  requests it or confirms it after PR creation. After the version branch is
  merged to `master`, run the Add Tag Workflow.
- Do not create or push a tag during the bump workflow.
- Do not decide SemVer, inspect release range, or edit version metadata during
  the tag workflow.

## Bump Version Workflow

Use this workflow to prepare a new release version commit and pull request. It
updates version metadata, commits the bump, pushes a version branch, and creates
the PR. It must not create or push release tags.

1. Check the repository state first.
   - Run `git status --short`.
   - If there are local changes, show the status output and ask whether the
     user wants to continue.
   - Explain that the bump branch will be created from `origin/master`.
     Uncommitted local changes may follow the checkout and contaminate the bump
     unless the user keeps them out of the commit.
   - Do not stash, reset, checkout, or discard changes unless the user gives a
     separate explicit instruction.
   - If continuing with a dirty worktree, keep unrelated changes out of the
     version bump commit.

2. Create a temporary branch from `origin/master`.
   - Run `git fetch origin --tags`.
   - Confirm the fetched base:
     - `git rev-parse --verify origin/master`
     - `git rev-parse --short origin/master`
   - Create and switch to a temporary branch from the fetched remote branch:
     - `git switch --create bump-version/tmp-<origin-master-short-sha> origin/master`
   - Confirm the current branch and target:
     - `git branch --show-current`
     - `git rev-parse --short HEAD`
   - The temporary branch must point at `origin/master` before version metadata
     is changed. Do not update local `master`, merge, rebase, reset, or rewrite
     history as part of this workflow.
   - If the fetch or branch creation fails because of network permissions,
     existing branch names, divergence, or local changes that would be
     overwritten, stop and report the exact failure.

3. Validate the current release metadata.
   - Run `node scripts/check-release.mjs`.
   - If it fails before the bump, stop and report the failure. Do not bump from
     inconsistent metadata unless the user explicitly asks to repair it.
   - Treat the version accepted by this script as the current local version.

4. Determine the next SemVer version.
   - If the user gave an explicit target version or bump type, use it after
     checking that it is a valid SemVer increase.
   - Treat major bumps as exceptional. Do not choose a major bump
     automatically; use one only when the user explicitly requests it and the
     SemVer increase is valid.
   - Otherwise derive the comparison baseline from the current version accepted
     by `scripts/check-release.mjs`.
   - Prefer the exact tag for the currently recorded version:
     - `git rev-parse -q --verify refs/tags/v<current-version>`
     - `git rev-parse -q --verify refs/tags/<current-version>`
     - `git ls-remote --tags origin v<current-version>`
     - `git ls-remote --tags origin <current-version>`
   - If the exact tag exists remotely but not locally, fetch that tag before
     comparing. If network access is blocked, request permission instead of
     guessing from an incomplete local tag set.
   - If one of those tags exists, use it as `<baseline-tag>`. Prefer the `v`
     prefixed tag when both exist.
   - If neither tag exists, report that the current version tag is missing and
     inspect the latest SemVer tag only as a fallback:
     - `git tag --list --sort=-v:refname`
     - `git describe --tags --abbrev=0`
   - If no SemVer fallback tag exists, stop and ask the user to choose the
     comparison baseline.
   - Inspect the changes between `<baseline-tag>` and the current HEAD:
     - `git log --oneline <baseline-tag>..HEAD`
     - `git diff --stat <baseline-tag>..HEAD`
     - `git diff --name-status <baseline-tag>..HEAD`
   - Use the diff content first and the amount of change as a supporting signal.
     A large internal-only diff can still be patch; a smaller diff that adds a
     user-visible capability can be minor.
   - Minor bump: user-visible features, new commands, new options, additive CLI
     behavior, new package/platform support, or backward-compatible capability
     additions. Broad multi-module changes that materially expand behavior also
     support minor.
   - Patch bump: bug fixes, docs, tests, performance work, refactors, release
     metadata fixes, dependency updates, and other compatible maintenance
     changes that do not add a new user-visible capability.
   - Breaking-change markers such as `BREAKING CHANGE`, `!` in a Conventional
     Commit header, public CLI contract breaks, or incompatible API changes are
     not automatic major bumps. Stop, report the evidence, and ask whether the
     user wants to make an explicit major bump or keep the release compatible
     with a minor/patch bump.
   - If the signal is mixed or uncertain, choose the smallest defensible bump
     and state the reason.
   - Before editing, present the current version, proposed version, bump type,
     baseline tag, comparison summary, reason, temporary branch, final version
     branch, and files expected to change. Wait for explicit user confirmation.

5. Rename the temporary branch to the version branch.
   - Unless the user gives a different branch name, use
     `bump-version/v<new-version>` as the final branch name. This keeps the
     release version visible while avoiding ambiguity with the later release tag
     `v<new-version>`.
   - Before renaming, check whether the final branch already exists:
     - `git rev-parse -q --verify refs/heads/bump-version/v<new-version>`
     - `git ls-remote --heads origin bump-version/v<new-version>`
   - If the branch already exists locally or remotely, stop and ask whether to
     use a different branch name or update the existing branch.
   - Rename the local branch after the user confirms the version bump:
     - `git branch -m bump-version/v<new-version>`
   - Confirm with `git branch --show-current`.

6. Update all release version metadata.
   - Update these files to the same new version:
     - `Cargo.toml`
     - `Cargo.lock` entry for the `codex-ops` package
     - `package.json`
     - `package-lock.json`
     - every `npm/*/package.json` platform package manifest
   - In `package.json` and `package-lock.json`, update all
     `@codexops/*` optional dependency versions to the same new version.
   - Prefer structured JSON/TOML-aware edits or package manager commands where
     practical. Avoid broad string replacements that could change unrelated
     dependency versions.

7. Validate the bumped metadata.
   - Run `node scripts/check-release.mjs`.
   - If the script fails, fix the version metadata and rerun it before
     committing.
   - Run `git diff --check`.
   - Optionally run broader checks if the user requested them or the bump
     touched more than release metadata.

8. Review, commit, push, and open the version bump PR.
   - Show the final diff summary and the exact version change.
   - Stage only the intended version metadata files.
   - Do not stage unrelated dirty files.
   - Commit with:
     - `chore: bump version to <version>`
   - After committing, push the version branch:
     - `git push -u origin bump-version/v<new-version>`
   - Create a pull request from the pushed branch to `master`:
     - `gh pr create --base master --head bump-version/v<new-version> --title "chore: bump version to <new-version>" --body "Bump codex-ops release version to <new-version>."`
   - If `gh pr create` reports that a pull request already exists for the
     branch, run `gh pr view bump-version/v<new-version>` and use that existing
     PR instead of creating another one.
   - If `gh` is not authenticated, unavailable, or blocked by network
     permissions, stop and report the exact failure.
   - Show the new commit SHA, pushed branch, and PR URL/number.

9. Optionally wait for checks and merge the PR.
   - Only do this if the user explicitly requested check-waiting/merge before
     the workflow started, or if the required PR creation step has finished and
     the user explicitly confirms continuing.
   - If the user did not already request it, ask whether to wait for PR checks
     and merge the PR after they pass.
   - To wait for checks, run:
     - `gh pr checks --watch`
   - If any required check fails, is cancelled, or remains inconclusive, stop
     and report the status. Do not merge.
   - Before merging, confirm the PR still targets `master`, the head branch is
     `bump-version/v<new-version>`, and the expected commit SHA is still the PR
     head.
   - Merge with a normal merge commit, not squash:
     - `gh pr merge --merge --delete-branch`
   - Do not use `gh pr merge --squash`; squash creates a new squashed commit and
     does not preserve the bump commit as-is.
   - If the repository does not allow merge commits or branch protection blocks
     the merge, stop and report the exact failure instead of switching merge
     strategy automatically.
   - After merge, remind the user that the release tag should be created
     separately with the Add Tag Workflow.

## Add Tag Workflow

Use this workflow for creating and pushing a release tag after the version bump
has already been merged to `master`. It must not inspect release ranges, decide
SemVer, or edit version metadata.

1. Check the repository state first.
   - Run `git status --short`.
   - If there are any tracked, untracked, staged, or unstaged changes, show the
     status output and explain the impact precisely: the tag will point only to
     a committed target on `master`, so uncommitted local changes will not be
     included in the tag, but those changes can block or complicate switching to
     `master` and fast-forwarding from `origin/master`.
   - Ask the user whether to continue with local changes present before
     attempting the branch switch or sync.
   - Do not continue past this step until the user explicitly confirms they want
     to continue with the dirty worktree.
   - Do not stash, reset, checkout, or discard changes unless the user gives a
     separate explicit instruction.

2. Switch to the release branch.
   - Run `git switch master`.
   - Confirm the current branch with `git branch --show-current`.
   - If `master` does not exist, stop and ask the user how to proceed.

3. Sync the latest release branch from the remote.
   - Run `git fetch origin`.
   - Run `git pull --ff-only origin master` or equivalently fast-forward the
     local `master` to `origin/master`.
   - If the fetch or fast-forward needs network permission, request it.
   - If fast-forward sync fails because local commits diverged or local changes
     would be overwritten, stop and report the exact failure instead of merging,
     rebasing, stashing, resetting, or discarding changes.
   - Confirm the synced target with `git rev-parse --short HEAD` and
     `git log -1 --pretty=format:%s`.

4. Validate the local release version.
   - Run `node scripts/check-release.mjs`.
   - If the script fails, stop and report the failure. Do not infer a tag from
     unchecked package metadata.
   - Use the script-validated local version from the success output
     `release metadata check passed for codex-ops <version>` as the source of
     truth for the expected tag.
   - For this repository, the expected release tag is `v<version>`.

5. Check whether the expected tag already exists.
   - Verify both local and remote tag state for `v<version>`:
     - `git rev-parse -q --verify refs/tags/v<version>`
     - `git ls-remote --tags origin v<version>`
   - If the version's tag already exists locally or remotely, tell the user
     exactly where it exists and stop before proposing or creating a replacement
     tag.

6. Show the target information and wait.
   - Before creating any tag, present:
     - current branch
     - remote sync status
     - target commit SHA and subject
     - local change status, especially if the worktree is dirty, with a note
       that dirty changes are not tag contents but may affect checkout/sync
     - release check command and validated local version
     - expected new tag from the validated local version
     - remote that will receive the tag, normally `origin`
     - exact commands that will be run
   - Ask the user to confirm the exact tag.
   - Do not run `git tag` or `git push` until the user explicitly confirms.

7. Create and push the tag after confirmation.
   - Re-check `git status --short`.
   - If local changes are present and the user has not already confirmed that
     exact dirty state, show the status output and ask again before continuing,
     explaining that the risk is checkout/sync interference rather than tag
     contents.
   - If the dirty state changed after the user's earlier confirmation, show the
     new status and wait for explicit confirmation before continuing.
   - Re-check `git branch --show-current`; stop if it is no longer `master`.
   - Re-sync with the remote before creating the tag:
     - `git fetch origin`
     - `git pull --ff-only origin master`
   - If the sync changes `HEAD` from the commit the user confirmed, stop,
     present the new target information, and wait for a fresh confirmation.
   - Re-run `node scripts/check-release.mjs`.
   - If the validated local version or expected tag changed after confirmation,
     stop and present the new target information for fresh confirmation.
   - Verify the expected tag still does not exist locally or remotely:
     - `git rev-parse -q --verify refs/tags/v<version>`
     - `git ls-remote --tags origin v<version>`
   - If the expected tag now exists, tell the user and stop.
   - Create an annotated tag:
     - `git tag -a v<version> -m "Release v<version>"`
   - Push only that tag:
     - `git push origin v<version>`

8. Wait for the Release workflow and report the result.
   - The tag push is expected to trigger `.github/workflows/release.yml`, named
     `Release`, because it listens for pushed tags matching `v*`.
   - Find the workflow run for the tag and target commit:
     - `gh run list --workflow release.yml --event push --branch v<version> --limit 5 --json databaseId,status,conclusion,headBranch,headSha,url,createdAt`
   - Prefer the run whose `headBranch` is `v<version>` and whose `headSha`
     matches the target commit SHA confirmed before tagging. If the run is not
     visible immediately, poll for a short bounded period and then report that
     the tag was pushed but the Release run was not found yet.
   - When the run is found, wait for it to finish:
     - `gh run watch <run-id> --exit-status`
   - After it finishes, show the final run status and URL:
     - `gh run view <run-id> --json status,conclusion,url,headBranch,headSha,createdAt,updatedAt`
   - If the run fails, is cancelled, or does not complete successfully, show the
     failure summary and fetch failed logs when practical:
     - `gh run view <run-id> --log-failed`
   - If `gh` is unavailable, unauthenticated, or blocked by network permissions
     after the tag has already been pushed, report that the tag push completed
     and that Release workflow waiting could not be performed.

## Safety Rules

- Never create or push a Git tag in the bump workflow.
- Never inspect release range, decide SemVer, or edit version metadata in the
  tag workflow.
- Never publish npm or Cargo packages in this workflow.
- Never push directly to `master` during the bump workflow.
- Never use `git push --tags`.
- Never merge a bump PR unless the user explicitly requested it or confirmed it
  after PR creation.
- Never use squash merge for the bump PR unless the user explicitly asks for a
  separate policy change.
- Never force-push, force-update tags, delete tags, or rewrite history unless
  the user explicitly asks for a separate recovery operation.
- Stop if the proposed version is not greater than the current version.
- Stop if the target version's tag already exists locally or remotely:
  - `git rev-parse -q --verify refs/tags/v<version>`
  - `git ls-remote --tags origin v<version>`
- Stop if the final version branch already exists locally or remotely unless
  the user explicitly chooses how to handle it.
- If network access or permissions are blocked during fetch, push, or remote
  tag/branch lookup, request the needed permission rather than guessing.
- If release metadata and the expected tag disagree, stop and ask whether the
  metadata or tag target should be corrected before tagging.
- After pushing a release tag, wait for the `Release` GitHub Actions workflow
  when `gh` is available, then report the conclusion and run URL.

## Bump Confirmation Template

Use a concise confirmation message before editing:

```text
Ready to bump release version.

Base: origin/master
Temporary branch: bump-version/tmp-<origin-master-short-sha>
Final branch: bump-version/v<new-version>
Current version: <current-version>
Proposed version: <new-version>
Bump: <minor|patch> because <reason>
Baseline tag: <baseline-tag>
Compared range: <baseline-tag>..HEAD
Target tag after bump: v<new-version>
Expected files:
Cargo.toml
Cargo.lock
package.json
package-lock.json
npm/*/package.json

Validation after edit:
node scripts/check-release.mjs
git diff --check

Commit after validation:
chore: bump version to <new-version>

Push and PR after commit:
git push -u origin bump-version/v<new-version>
gh pr create --base master --head bump-version/v<new-version> --title "chore: bump version to <new-version>" --body "Bump codex-ops release version to <new-version>."

Optional after PR creation, only with explicit confirmation:
gh pr checks --watch
gh pr merge --merge --delete-branch

Please confirm the version bump before I edit files.
```

## Tag Confirmation Template

Use a concise confirmation message before creating the tag:

```text
Ready to create release tag.

Branch: master
Remote sync: origin/master at <short-sha>
Target: <short-sha> <commit subject>
Local changes: <clean|status summary>
Release check: node scripts/check-release.mjs passed for codex-ops <version>
Expected tag: v<version>
Remote: origin

Commands after confirmation:
git fetch origin
git pull --ff-only origin master
node scripts/check-release.mjs
git tag -a v<version> -m "Release v<version>"
git push origin v<version>
gh run list --workflow release.yml --event push --branch v<version> --limit 5 --json databaseId,status,conclusion,headBranch,headSha,url,createdAt
gh run watch <run-id> --exit-status
gh run view <run-id> --json status,conclusion,url,headBranch,headSha,createdAt,updatedAt

Please confirm the exact expected tag to create.
```

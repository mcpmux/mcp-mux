# Changesets

This folder is used by [Changesets](https://github.com/changesets/changesets) to manage versioning and changelogs.

## Adding a changeset

```bash
pnpm changeset
```

Follow the prompts to describe your changes. This creates a markdown file in this folder.

## Release process

1. PRs with changesets get merged to main
2. The release workflow creates a "Version Packages" PR
3. Merging that PR triggers the release build

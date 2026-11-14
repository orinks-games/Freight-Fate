# Channel source branches

This reusable Hearken action resolves a channel to a configured source branch. It supports stable, beta, alpha, and custom channels. It neither switches a checkout nor builds or publishes anything. It needs Node.js and Git, available on GitHub-hosted runners.

```yaml
- uses: orinks-games/hearken/.github/actions/update-source@<reviewed-commit>
  id: source
  with:
    channel: ${{ inputs.channel || 'beta' }}
    channel-branches: '{"stable":"main","beta":"dev","alpha":"career/2.0"}'
    branch: ${{ inputs.branch }}
- uses: actions/checkout@v4
  with:
    ref: refs/heads/${{ steps.source.outputs.branch }}
    fetch-depth: 0
```

Replace `<reviewed-commit>` with a published, reviewed commit SHA. Pin the resulting checkout's `git rev-parse HEAD` for every platform build. Publish archives and manifests signed for the selected channel through the game's existing release pipeline. A branch override affects this run only. Missing mappings and invalid names fail; there is no fallback to another channel. Explicit stable release tags should keep the caller's existing tag-based path.

A feature branch must contain the current packaging integration and use the intended upcoming package version. Selecting alpha does not make an older version an upgrade or make incompatible saves portable. Players select signed release channels; they do not download or execute branch source. Merely configuring an alpha mapping does not schedule or publish alpha builds.

Run `node --test .github/actions/update-source/resolve.test.cjs` from the repository root.

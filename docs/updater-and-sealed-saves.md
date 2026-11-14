# Release channels and sealed careers

Players choose stable, beta or alpha in Settings. Existing dev preferences follow beta. Beta accepts newer beta and stable builds; alpha accepts newer alpha, beta and stable builds. The chosen subscription persists after installing a steadier build. All candidates must pass the existing signature, game, platform and version checks. Old copies still receive separately signed dev manifests alongside beta manifests.

## Branch builds

The snapshot workflow accepts a channel and optional branch. It uses Hearken's shared `update-source` action, copied locally with commit provenance until the new engine commit is published. The resolver validates names and mappings without checking out or publishing anything; the existing prepare job pins the selected source commit for all platforms. Beta defaults to dev; alpha defaults to feat/career-2.0 (the actual remote branch name). Dev builds (each push to dev, and a nightly fallback) publish as Beta prereleases and never take GitHub's Latest slot. Stable releases still require an explicit version tag, publish signed stable-channel manifests, and become GitHub's Latest release. The `1.9-tester-YYYYMMDD` tags remain for compatibility with older installed updaters. Alpha uses separate timestamped release tags, so it cannot replace the daily beta snapshot. The updater installs signed release archives, never a Git checkout.

Before publishing alpha, integrate this updater, packaging code and the identical workflow into the chosen source branch. The workflow's existing stale-file check enforces this. Use a dry run first. Set the source branch workspace version to its intended upcoming release line (for example, 2.0.0) before publishing alpha. An alpha built with an older package version will not be offered over newer stable or beta versions. Selecting a channel does not migrate careers between incompatible release lines or permit a version downgrade.

## Local save sealing

The profile save path wraps the existing compressed, HMAC-signed container in Hearken's authenticated encryption. Legacy careers retain their existing integrity checks before migration. Migration preserves a plain-backup copy. A witness prevents a plain replacement being accepted after a slot was sealed. Locked or forged files are left intact; use the released game or a verified cloud restore to recover.

Cloud upload content and driver-board validation remain portable profile JSON. Server signatures are verified before restoring; the restored profile is locally signed and sealed. Local sealing is not proof of honest play and does not replace server-side validation.

## Activation and key recovery

Freight Fate now has a persistent seal-id and repository secret configured. The key is backed up in Dropbox under `Hearken key backup/seal-keys/freight-fate.key`; retain it for future releases.

For a new game only, run `cargo tools seal-key --new` once with owner approval. Back up the generated private key securely, record only its printed seal-id under workspace.metadata.release, and provision the file's contents as the repository HEARKEN_SEAL_KEY secret. Never commit or log the key. Desktop and iOS release packaging checks its ID before compiling; a missing or different key fails the build once seal-id is configured. Without seal-id, packaging deliberately remains unsealed.

Retain the same key for every platform and release channel. A new key cannot read existing sealed careers. Developer builds do not seal and refuse to overwrite sealed careers. Cloud backups remain the supported cross-device restore path. Older released versions cannot open the new sealed format; preserve migration backups when testing rollback.

The pinned seal crate is vendored with a filename-policy extension for existing careers containing spaces and Unicode. Its format and cryptography are unchanged; see `vendor/hearken-seal/README.md` for provenance and removal guidance.

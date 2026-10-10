# Freight Fate on TestFlight

A manual GitHub Actions workflow builds the iOS game, signs it and uploads it
to TestFlight for internal testers. The workflow file is
.github/workflows/ios-testflight.yml. It never runs by itself: no push, tag or
pull request starts it, and it makes no tag and no release.

## Run it

Dispatch it from dev. The upload input decides what the run does:

- upload off (the default): a verification run. It checks the key, makes the
  temporary certificate and profile, builds and signs the app, then deletes the
  certificate and profile. It uploads nothing to TestFlight, creates nothing
  else in App Store Connect, and the run summary says so. The signed ipa is kept
  as a workflow artifact for one day. It holds no key or password.
- upload=true: the same, then the signed build goes to TestFlight.

Recommended first run, verification only:

    gh workflow run ios-testflight.yml -R orinks-games/Freight-Fate --ref dev

Then, after the touch gesture redesign (#340) is merged into dev, a real upload:

    gh workflow run ios-testflight.yml -R orinks-games/Freight-Fate --ref dev -f upload=true

GitHub can only dispatch a workflow whose file is on the default branch, so the
workflow file has to reach main once before the first run. Watch a run with:

    gh run list --workflow ios-testflight.yml -R orinks-games/Freight-Fate
    gh run watch -R orinks-games/Freight-Fate

Any ref other than dev stops at the first step unless you pass
-f allow_other_ref=true, so a build from unmerged work is always on purpose.
Every run prints the branch and commit SHA it built, writes them to the run
summary, and sends them to TestFlight as the build's release notes.

## Identity: where the numbers come from

- Bundle id: net.orinks.freightfate, set only in tools/build_ios.py (BUNDLE_ID).
  The workflow and tools/ios_signing.py read it from there. The app record must
  use the same bundle id.
- Version (CFBundleShortVersionString): the project version in pyproject.toml
  cut to three numbers, so 1.9.0.dev0 is 1.9.0. To ship a new version, change
  pyproject.toml; the run refuses a version lower than one TestFlight already has.
- Build number (CFBundleVersion): the run asks App Store Connect for the highest
  number it has for the app and uses max(that, run number) + 1, so it continues
  the earlier builds. To choose it yourself pass -f build_number=40; it must be
  higher than every earlier upload of the same version.
- Device family: whatever the app's Info.plist says (iPhone and iPad).

It uses a standard macos-26 runner, which is free for this public repository.

## What you do once

The app record already exists in App Store Connect. If the run cannot find it,
it stops before building and says so.

1. Check the API key access. In Users and Access, Integrations, App Store
   Connect API, the key named by the ASC_KEY_ID secret must have App Manager or
   Admin access. Admin is simplest. A Developer key cannot make certificates.
2. Make sure you are an internal tester: App Store Connect, the app, TestFlight,
   Internal Testing, a group with your Apple ID. Install the TestFlight app on
   the iPhone and sign in with the same Apple ID.

The repository secrets APPLE_TEAM_ID, ASC_ISSUER_ID, ASC_KEY_ID and
ASC_KEY_P8_BASE64 are used as they are. No other secret is needed.

## What the workflow does

1. Writes the App Store Connect key into a temporary folder and hides it from the logs.
2. tools/ios_signing.py makes a private key and a certificate request, registers
   the bundle id if it is new, creates a temporary iOS distribution certificate
   and an App Store provisioning profile that names it.
3. Imports the certificate into a throwaway keychain and installs the profile.
4. Checks that the app record exists and finds the next build number.
5. Runs tools/build_ios.py --device --ipa, which builds with Rust, signs and
   packages FreightFate.ipa.
6. Only when upload is true, uploads it to TestFlight with the export compliance answer "no non-exempt
   encryption", matching ITSAppUsesNonExemptEncryption in Info.plist, and waits
   for Apple to finish processing.
7. Always, even after a failure: deletes the temporary profile and certificate
   through the API, the keychain and every key file, so runs never use up
   Apple's limit on distribution certificates.

## How the first run fails if setup is missing

Each failure prints one plain error line naming the fix.

- App record missing: "There is no App Store Connect app record for bundle id
  net.orinks.freightfate". Its bundle id must match the one in
  tools/build_ios.py. Nothing was built.
- Version too low: TestFlight has a higher version than pyproject.toml; raise it.
- Key without permission (HTTP 403): "The App Store Connect API key is not
  allowed to do this". Give the key App Manager or Admin access.
- Key rejected (HTTP 401): the three ASC secrets do not belong to one live key.
- Certificate limit: Apple will not make another distribution certificate.
  Revoke an old one nothing uses at developer.apple.com, Certificates.
- If the cleanup step cannot delete the temporary certificate, it prints a
  warning with the id. Delete that certificate by hand at developer.apple.com.

No log line prints a key, a token, a certificate or a profile.

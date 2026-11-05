# Freight Fate on TestFlight

A manual GitHub Actions workflow builds the iOS game, signs it and uploads it
to TestFlight for internal testers and, by default, the external Friends
group. The workflow file is
.github/workflows/ios-testflight.yml. It never runs by itself: no push, tag or
pull request starts it, and it makes no tag and no release.

## Run it

Dispatch it from dev. The upload input decides what the run does:

- upload off (the default): a verification run. It checks the key, the stored
  certificate and its profile, then builds and signs the app. It uploads nothing
  to TestFlight, changes nothing in App Store Connect, and the run summary says so. The signed ipa is kept
  as a workflow artifact for one day. It holds no key or password.
- upload=true: the same, then the signed build goes to TestFlight.

Who gets an uploaded build is the testers input:

- testers=friends (the default): the Internal group gets the build as soon as
  Apple finishes processing it. The run also adds it to the external Friends
  group (the public TestFlight link) and submits it to Apple's beta review.
  Friends get it once Apple approves it, usually within a few hours; later
  builds of an approved version often pass at once.
- testers=internal: only the Internal group gets it, for a build you want to
  try yourself first. Send it to Friends later in App Store Connect
  (TestFlight, the build, Groups) or with
  `cargo tools ios-signing distribute --build-number N`.

The whats_new input is the "What to Test" note testers read in the TestFlight
app. Leave it empty and the note is the branch and commit, which means nothing
to Friends, so write one or two plain sentences for a build that goes to them:

    gh workflow run ios-testflight.yml -R orinks-games/Freight-Fate --ref dev \
      -f upload=true -f whats_new="New touch controls: a flick is one step."

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

- Bundle id: net.orinks.freightfate, set only in cargo tools build-ios (BUNDLE_ID).
  The workflow and cargo tools ios-signing read it from there. The app record must
  use the same bundle id.
- Version (CFBundleShortVersionString): the project version (package-version
  under [workspace.metadata.release] in the root Cargo.toml) cut to three
  numbers, so 1.9.6.dev0 is 1.9.6. To ship a new version, change it there; the
  run refuses a version lower than one TestFlight already has.
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
   Admin access. Admin is simplest. A Developer key cannot make profiles.
2. Make sure you are an internal tester: App Store Connect, the app, TestFlight,
   Internal Testing, a group with your Apple ID. Install the TestFlight app on
   the iPhone and sign in with the same Apple ID.

The repository secrets APPLE_TEAM_ID, ASC_ISSUER_ID, ASC_KEY_ID and
ASC_KEY_P8_BASE64 hold the App Store Connect key. IOS_DISTRIBUTION_P12_BASE64
and IOS_DISTRIBUTION_P12_PASSWORD hold the Apple Distribution certificate and its
private key as a .p12 (base64) and that file's password. Every run signs with
this one certificate, so no run creates or revokes a certificate and Apple sends
no "certificate revoked" mail.

## Replacing the stored certificate

The certificate lasts a year (the current one expires on 8 October 2027). When
it expires or is revoked, the run stops with "The stored distribution
certificate ... is not on the team" or "... expired". To replace it, make a new
Apple Distribution certificate from a certificate request for the same private
key (developer.apple.com, Certificates, or the App Store Connect API), then
build the .p12 in the format the macOS keychain accepts and store both secrets:

    openssl pkcs12 -export -legacy -inkey key.pem -in cert.pem -out dist.p12

The run makes a new App Store profile for the new certificate by itself the
first time, and reuses it after that.

## What the workflow does

1. Writes the App Store Connect key into a temporary folder and hides it from the logs.
2. Imports the stored certificate and key into a throwaway keychain.
3. cargo tools ios-signing finds that certificate on the team by its serial number,
   registers the bundle id if it is new, and reuses the active App Store
   provisioning profile that names the certificate. Only when there is none
   does it make one, which later runs then reuse. The profile is installed.
4. Checks that the app record exists and finds the next build number.
5. Runs cargo tools build-ios --device --ipa, which builds with Rust, signs and
   packages FreightFate.ipa.
6. Only when upload is true, uploads it to TestFlight and waits for Apple to
   finish processing. The export compliance answer ("no non-exempt encryption")
   comes from ITSAppUsesNonExemptEncryption in Info.plist; the workflow does not
   set it again, because Apple rejects a second update with HTTP 409.
   With testers=friends it then adds the build to the Friends group and submits
   it for beta review.
7. Always, even after a failure: deletes the keychain and every key file on the
   runner. Nothing in App Store Connect is deleted or revoked.

## How the first run fails if setup is missing

Each failure prints one plain error line naming the fix.

- App record missing: "There is no App Store Connect app record for bundle id
  net.orinks.freightfate". Its bundle id must match the one in
  cargo tools build-ios. Nothing was built.
- Version too low: TestFlight has a higher version than pyproject.toml; raise it.
- Key without permission (HTTP 403): "The App Store Connect API key is not
  allowed to do this". Give the key App Manager or Admin access.
- Key rejected (HTTP 401): the three ASC secrets do not belong to one live key.
- Stored certificate gone or expired: replace it as described above.
- No Friends group: "There is no external TestFlight group named 'Friends'".
  The build was still uploaded for internal testers.
- Keychain refused the .p12: the password secret does not match, or the file
  was written without -legacy.

No log line prints a key, a token, a certificate or a profile.

# Hearken shared action

This copy is identical to `.github/actions/update-source` at Hearken commit `3e44aef6404f410398e0a323dc7ce02ef756f90f` (MIT OR Apache-2.0). That engine commit is prepared locally and not yet published. The local copy keeps Freight Fate builds self-contained until it is available upstream.

Update the resolver in Hearken first, copy the reviewed action here, and run its tests in both repositories. Once that commit is published, callers may use the pinned Hearken action directly instead. Branch defaults belong to each game's workflow, not to this shared resolver.

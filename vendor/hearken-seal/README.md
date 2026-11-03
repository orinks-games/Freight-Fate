# Freight Fate filename compatibility

Vendored from Hearken commit `a9d452a76365ce0e933050611d02d65fd616617a`, crate `hearken-seal`. Original MIT/Apache licenses are included.

The only behavioral change permits the Unicode letters/numbers and spaces already used by Freight Fate career filenames, with a 240-byte limit leaving room for backup suffixes. Traversal, separators, leading dots/spaces, trailing dots/spaces, reserved vault suffixes, and other punctuation remain rejected. Format, encryption, key derivation, witness rules, and atomic writes are unchanged. Cargo metadata is made standalone.

Remove the Cargo patch and this copy when an upstream revision supports these filenames. Run `cargo test -p hearken-seal` alongside profile migration tests when updating it.

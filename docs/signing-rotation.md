# Signing and release channels

How Susurro ships trusted updates, and what happens when a key must change.

## Channels

Stable is the default. The app checks `latest.json` quietly from settings
and never forces a modal. Tags containing `beta` publish as prereleases
through the same pipeline; stable tags publish as full releases. The
update channel setting picks which line the app follows.

## Keys

Two halves, two places. The private key lives only in the
`TAURI_SIGNING_PRIVATE_KEY` GitHub secret (plus its password secret) and
signs every bundle at release time. The public key lives in
`app-tauri/src-tauri/tauri.conf.json` under `updater.pubkey` and verifies
every download on the machine. Every shipped asset carries a `.sig` file
beside it. A local copy of the signing keypair may exist at
`~/.local/share/susurro/tauri-signing.key` for maintainer use; it never
enters the repo.

## Rotation cadence

Rotate once a year, or immediately on suspected exposure. Either way the
procedure is the same, so the yearly run keeps the emergency path warm.

## Rotation procedure

1. Generate a fresh keypair with `tauri signer generate`.
2. Put the new public key into `tauri.conf.json` and cut a bridge
   release signed with the old private key. Old clients accept it,
   and it carries the new public key forward.
3. Put the new private key plus password into the GitHub secrets,
   replacing the old values.
4. Cut the next release, signed with the new key. From here on,
   clients verify against the new key.
5. Delete the old private key everywhere it lived.

Never ship a release signed by a key whose public half is not
already in the previous release. The bridge release is what makes
that true.

## Asset manifest key

Model and asset files verify against a second Ed25519 keypair,
`susurro-assets-1`, owned by the provisioning plane (`provision/`,
ADR-004). The public half is embedded as `ASSET_PUBLIC_KEY_HEX` and
verifies every remote manifest; the secret half signs manifests at
release time and lives in the password manager (later the release
secrets beside `TAURI_SIGNING_PRIVATE_KEY`). It never enters the repo.
Rotation follows the same bridge procedure: new key id, new embedded
public key in a release the old clients accept first, then manifests
move to the new id. Old clients reject the unknown key id loudly
instead of trusting bytes they cannot verify.

## Emergency rotation

On exposure, rotate first and announce second: run the procedure above,
then post what was exposed, which releases it touched, and the new key
fingerprint. Users on an affected version update twice if needed, once
to a clean bridge release and once to the rotated release.

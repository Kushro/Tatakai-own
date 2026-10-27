# Third-party notices (B4, m3e/android-media)

Code adapted from the author's own repositories (AGPL-3.0-or-later), per decision 1 of docs/redesign/06-user-decisions.md:

- `core/link/.../LinkCrypto.kt` (LinkCrypto, LinkCsr, Der), `PairingPayload.kt`, `LinkHttp.kt`, `LinkError.kt` (structure), `PairingClient.kt`:
  adapted from Enigmata `app/src/main/java/app/enigmata/temple/{crypto,pairing,client}` (TempleCrypto, TempleCsr, TemplePairingPayload, TempleHttp, TempleError, TemplePairingClient).
- `core/link/.../LinkKeyVault.kt`: key-sealing model (software P-256 key sealed with an AndroidKeyStore AES-GCM key under `noBackupFilesDir`) from Enigmata `temple/vault/{TempleKeyVault,KeystoreTempleKeyCipher}`.

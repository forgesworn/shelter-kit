# Changelog

## 0.2.2 - 2026-08-30

- Build reqwest with `rustls-no-provider` and install ring as the process
  CryptoProvider in the fetch adapters, so a node carries exactly one TLS
  backend end to end (joint core ledger, finding 15).  A host that installs
  another provider first still wins: install_default is first-come.

## 0.2.0 - 2026-08-30

- Supply the storage quota at runtime through `AppState::set_quota`, applied
  at the same reconcile boundary as the owner set and friend grants, and
  refused with the requested and currently-held byte counts when the pool
  already holds more than the new figure allows for (joint core contract
  0.2 §3, item B13). Shrinking the pool below its low watermark evicts
  guest claims once, immediately, at that boundary.
- Supply the owner set and the friend grants at runtime through
  `AppState::set_owner_keys` and `AppState::set_friend_grants`, applied at a
  reconcile boundary that never lands mid-stream against an in-flight upload
  (joint core contract 0.2 §3).
- Add `FriendGrant::issuer`, so revoking every grant from one issuer is a
  single `set_friend_grants` call.
- Add `AppState::reconcile_now` and `BlossomConfigError::Task`.
- Carry a nullable, advisory `class` on every claim and reservation, set from a
  `class` tag on the authorising event and returned in `/list` descriptors.  It
  never influences the retention tier, eviction or serving (joint core contract
  0.2 §3, §5).  Schema `user_version = 3`: an added column, so every blob and
  claim migrates forward in place.
- Accept a URL-shaped BUD-11 `server` tag, not only a bare domain, and match
  it against a configured accepted name of either shape (joint core contract
  item B12). `rust-nostr`'s `TagStandard::Server` -- and so `stash-rs` --
  mints a full URL; a node configured with a bare name was rejecting a
  genuine token outright.

## 0.1.2 - 2026-08-29

- Accept standard base64 in BUD-11 authorisation, not only url-safe. BUD-01
  specifies standard base64; the decoder now reads both alphabets, padded or
  unpadded, so a spec-compliant client and stash-rs verify while existing
  url-safe clients keep working.

## 0.1.1 - 2026-08-28

- Add a direct public-HTTPS mirror and repair adapter.
- Disable ambient proxies and redirects in direct mode.
- Filter direct DNS results so hostnames cannot resolve to non-public networks.
- Keep Tor and direct HTTPS as shell-selected transports over the same router,
  authorisation and content-addressed store.

## 0.1.0 - 2026-08-27

- Extract the reviewed Wildbloom Node storage and Blossom core.
- Provide an unbound BUD-01/02/04/06/11/12 router.
- Provide atomic content-addressed storage with owner, friend and guest claims.
- Provide transport-neutral mirror and repair through `BlobFetcher`.
- Keep BUD-01 product identity supplied and validated by each shell.
- Publish the joint implemented core contract and explicit non-goals.

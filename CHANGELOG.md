# Changelog

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

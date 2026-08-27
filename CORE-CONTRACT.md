# Shelter Kit core contract

This document defines the joint boundary shared by Wildbloom, Bothy and other
compatible shells.  It describes version 0.1.0 as implemented, not a proposed
future network.

## Ownership

Shelter Kit owns:

- BUD request parsing and Nostr signature, operation, server, expiry and hash
  validation;
- quota reservation before streaming bytes;
- exact length and SHA-256 verification;
- atomic content-addressed storage and SQLite metadata;
- signer claims and owner, friend or guest retention classification;
- self-only listing and claim-aware deletion;
- verified mirror-source recording and repair;
- startup reconciliation and integrity reporting; and
- the transport-neutral `BlobFetcher` interface.

A shell owns listeners, process lifecycle, node identity, transport selection,
configuration and user consent.  An application owns encryption keys,
publication events, catalogues, payment, desired replica count and the meaning
of the bytes.

## Storage transaction

An accepted write reserves its declared length before reading the body.  Bytes
stream to a temporary file while SHA-256 is calculated.  The core commits only
when the delivered length and digest are exact, then atomically installs the
blob in the content-addressed store and records the claim.  Failed and
interrupted writes release their reservation and never become claims.

Physical bytes are deduplicated by digest.  Claims are not: each signer retains
their own class, declaration and deletion authority over the shared blob.

## Retention classes

- `owner`: configured owner keys.  Accepted for direct upload and mirror and
  retained ahead of all other classes.
- `friend`: an explicit, expiring per-key grant with a byte ceiling.  Accepted
  for direct upload and mirror while the grant is valid.
- `guest`: unknown valid signers, admitted only through BUD-04 when open shelter
  is enabled and only above the guest watermark.  Guest data is best effort and
  oldest guests are evicted first.

Removing an owner or friend policy demotes affected claims during reconciliation;
it does not silently preserve a promise that no longer exists.

## Serving

Owner declarations may supply a safe content type.  A blob with only friend or
guest claims is served as `application/octet-stream` with attachment and
`nosniff`.  Conflicting owner declarations also fail opaque.  A transport never
chooses or promotes a retention class.

## Mirror and repair

The router accepts only hash-addressed sources allowed by its source policy.  A
`BlobFetcher` returns a bounded byte stream, declared size, declared media type
and the path actually used.  The core independently enforces exact size and
digest before commit.

A source becomes a repair candidate only after serving verified bytes.  Repair
repeats the same exact-length and exact-hash checks.  A recorded source is not a
custody promise and may be offline when repair is needed.

## Versioning

The Rust API, storage schema, request validation and observable HTTP behaviour
are governed together by semantic versioning.  Compatible schema changes must
migrate forward without losing blobs or claims.  A change that weakens validation,
changes retention meaning or requires a destructive migration is a major-version
change and needs review by both publishing and shelter consumers.

## Explicitly outside 0.1.0

The following are not part of this version's contract: an encryption envelope,
vault or recovery keys, Nostr node claim/status events, peer discovery,
federation promises, replica manifests, proof of storage, paid quota, QUIC,
WebSocket relaying and platform listeners.  They must not be described as
shipped merely because the fetch interface can carry a future adapter.

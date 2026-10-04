# Paid storage core (0.5.0 prerelease)

This implementation adds operator-owned capacity commitments and a `paid`
retention tier. It does not receive money, verify settlement, expose checkout
routes or keep wallet credentials. A trusted selling shell calls the storage
API after its own durable payment verification. Do not expose these methods as
unauthenticated HTTP handlers.

## Local lifecycle

1. Persist the seller's immutable quote, including price, payment method, node,
   signer, storage and delivery terms, and refund policy in the operator's
   checkout service. Use a stable, opaque order ID; never use a bearer secret.
2. Call `Store::reserve_paid_sale` before offering payment. The storage portion
   binds that ID, signer, capacity, duration, grace and hold deadline. Holds last
   at most 24 hours. Conflicting replays fail; expired holds cannot be refreshed.
3. After authoritative settlement, call `Store::activate_paid_sale` with the
   same ID. Activation and its result are atomic. Repeating activation returns
   the original result, even after a later renewal. The first term starts at
   activation. The shell must persist settled-but-not-active orders for retry.
4. The existing Blossom router independently verifies the upload's BUD
   signature, server, operation, hash and expiry, then resolves the signer's
   allowance. Payment changes no authorisation header or Nostr event format.
5. A renewal is a new sale ID with `renews` referencing the stable allowance.
   Version one retains the same signer and ceiling. Its term extends from the
   later of the current write deadline or activation time. Renewal never
   shortens existing retention. Capacity upgrades and transfers are unsupported.

Only one allowance and one live pending sale per signer are supported. An old
allowance remains the renewal target after expiry. An expired hold cannot be
activated: the seller must arrange a newly reserved fulfilment or its refund
procedure. The core never creates a money balance or treats a timeout as a
failed payment. Sale records contain no price or settlement assertion; those
belong in the operator's durable checkout records.

## Capacity and retention

`StoreStats.bytes` and `reserved_bytes` report physical usage and streams.
`committed_bytes` additionally accounts for full sold ceilings and live quote
holds. New sales conservatively need available capacity; they do not trigger
guest eviction. Owner and friend writes cannot consume committed capacity.
Quota reductions and reopening with a smaller pool refuse to violate it.

Physical files remain deduplicated. Each paid signer pays their full logical
size against their own ceiling, including concurrent streams. Full ceilings
remain reserved even when files are deleted. Paid-only physical copies are
covered by these commitments; copies also claimed by non-paid users consume
separate capacity. This deliberately conservative accounting prevents deletion
of one paid claim from overselling its newly unused allowance. Adding a
non-paid claim to a paid-only copy can therefore be refused at a full pool.

Paid writes are checked before reading and again at commit. A stream crossing
its write deadline is refused unless renewal has extended the allowance.
Expiry stops writes; existing claims remain protected and listable through
the grace deadline. Reaping removes only expired claims, preserving other
signers' copies. Renewal and reaping use the same SQLite transaction boundary.
Renewal after grace cannot restore bytes already collected.

Owner/friend policy reconciliation and guest eviction cannot demote active
paid claims. A same-signer non-paid re-upload cannot silently replace paid
retention. A seller may still use the existing explicit policy tombstone to
remove content; commercial/legal removal and refund obligations stay with the
seller. Payment is neither replication evidence nor proof of future custody.

Paid-only responses remain opaque, like friend/guest data. Public Blossom
file descriptors contain no order IDs, allowance IDs or payment references.
Signer-authenticated listing includes paid recovery claims even if the signer
also has a friend grant. No new checkout authentication protocol is invented.

## Migration and release boundary

Schema 6 replaces the claim-tier constraint and adds durable sale and allowance
tables. Existing hashes, bytes, claims, class labels and verification evidence
are preserved. Interrupted upload reservations are still cleared on restart;
sale holds and activation records survive. Older schema-5 cores reject schema
6. Back up before migration; do not attempt a downgrade by editing the version.

This is the 0.5.0 prerelease. It adds a public enum variant and statistics
field, so consumers must review the minor-version upgrade and update their pin.
Wildbloom Node integration is a separate dependency update.
The local tests and temporary Node dependency override do not establish a
crates.io publication, deployed node, complete checkout or paid-service readiness.

Acceptance covers concurrent sales and uploads, exact replay, quota pressure,
deduplication, shared claim deletion, restart, renewal, grace, late activation,
commit-time expiry, schema-5 migration and signed Blossom upload/list/download.
Lightning, LNURLcash, Cashu, Bitcoin and Monero adapters, checkout authentication,
durable payment recovery, browser consent journeys and operator launch review
remain separate implementation work.

## Local validation, 4 October 2026

- `cargo test --locked`: 101 core/router tests passed.
- Core formatting and `cargo clippy --all-targets -- -D warnings` passed.
- A temporary copy of Wildbloom Node used the local Shelter path override:
  46 tests passed, including real loopback-node replica repair after restart.
  One real-Tor test was ignored because it requires an explicitly supplied
  Tor executable. Node lint and build passed.
- Wildbloom's production build and `acceptance:recovery` passed against that
  executable: fresh-browser recovery from a restarted replica after the
  original node stopped, relay and saved-event journeys, wrong-key/corrupt-copy
  refusal and no retained browser state.

The browser recovery test exercises the existing publication/recovery journey,
not a payment checkout. The Node source checkout and release pin were unchanged;
the dependency override existed only in the temporary compatibility build.

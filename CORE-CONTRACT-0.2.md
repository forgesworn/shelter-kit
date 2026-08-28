# Joint core contract -- 0.2

*The jointly agreed 0.2 contract for the shared Wildbloom / Bothy storage core
and its transport lanes, written on top of Shelter Kit `CORE-CONTRACT.md` 0.1.1.
Where this document exceeds 0.1.1 as implemented, it is the agreed target both
products build to; where it matches 0.1.1 it is adopted as written. It is owned
jointly: a change needs both owners.*

*Three parties, one boundary. The **core** (Shelter Kit) owns storage,
authorisation, retention, mirror and repair, and the `BlobFetcher` interface. A
**shell** (Wildbloom Node, Bothy) owns listeners, node identity and events,
transport selection and every eviction decision above `guest`. An
**application** (Wildbloom, V4V, Bothy's clients) owns encryption, catalogues,
payment and the meaning of the bytes. The storage half was drafted by Quill and
the transport half by Tally on 2026-08-28; the numbered ledger that produced
them (A1-A8 accepted, B storage delta, C transport questions, D build split) is
resolved but for one owner item recorded in §E below.*

*Bothy's names map one-to-one and are never transmitted: keeper = `owner`,
kith = `friend`, commons = `guest`.*

---

# Storage half

*The storage half of the joint Wildbloom / Bothy core contract, written on
top of `shelter-kit` `CORE-CONTRACT.md` 0.1.1. Every item in Bothy's delta
was answered by Tally on 2026-08-28 11:02 UTC (AGREED, with one scoping
counter on listing that this text adopts). Normative words: MUST / MUST NOT
/ MAY. The transport half is Tally's; the two halves merge into one
document beside `shelter-kit`, to be signed by both owners. The one owner item -- the envelope's review -- is recorded in section E.*

## 1. Ownership

The core owns: BUD request parsing and BUD-11 validation; reservation
before streaming; exact length and SHA-256 verification; atomic
content-addressed storage and SQLite metadata; signer claims and
owner / friend / guest retention; claim-aware deletion; listing; verified
mirror-source recording and repair; startup reconciliation and integrity
reporting; the `BlobFetcher` interface. Optionally, an admission
filter over the first bytes of a write.

A shell owns: listeners, process lifecycle, node identity and its events,
transport selection, configuration, user consent, the owner set
and friend grants at runtime, every eviction decision above
`guest`, tombstones.

An application owns: encryption keys and the envelope, publication events,
catalogues, payment, desired replica count and the meaning of the bytes.

## 2. Storage transaction

An accepted write MUST reserve its declared length before reading the body;
MUST stream to a temporary file while hashing; MUST commit only on exact
length and digest; MUST install atomically and record the claim; and MUST
release the reservation on any failure. Physical bytes are deduplicated by
digest; claims are per signer. If an admission filter is
configured, the core passes it the first 4 KiB after reservation and MUST
release the reservation and refuse the write on `Reject`. Supplied filters:
`None` (default) and `SealedParcelsOnly` (known media magic at offset 0, or
entropy below a floor, is rejected). The filter is a posture, not a proof,
and the contract says so.

## 3. Claims and retention

Retention classes are `owner`, `friend`, `guest` exactly as 0.1.1 defines
them. Bothy's *keeper / kith / commons* map onto them one-to-one and are
never transmitted.

A claim and a reservation carry a nullable `class` string set
from a `class` tag on the authorising event (upload) or pin; null means
"the shell's default". `class` MUST NOT influence retention tier, eviction
or serving inside the core; it is returned in list descriptors.

The owner set and the friend-grant set are supplied at runtime
through `set_owner_keys` and `set_friend_grants`, and MAY be re-supplied at
any time; a new set takes effect at the next reconcile boundary and MUST
NOT be applied mid-stream against an in-flight upload; removal demotes
affected claims at that reconciliation, exactly as 0.1.1 does for
configuration changes. How a shell obtains the
sets (configuration, a keeper-signed claim event, an invite) is the shell's.

A friend grant is `{ pubkey, ceiling_bytes, expires_at, grant_id,
issuer? }`. `issuer`, when present, is the pubkey that issued the grant;
a shell revoking every grant by an issuer is a `set_friend_grants` call.

The core MUST NOT delete or evict bytes that carry an `owner` or
`friend` claim, on its own, for either product -- a hard promise. When a `friend` claim's grant expires or is removed, the claim is
demoted to `guest`; the shell decides when a `guest` copy may go, using its
own verification. The core evicts only `guest`, oldest first, and only to
recover the reserve between its water-marks.

## 4. Authorisation

A BUD-11 event (kind `24242`, `Authorization: Nostr <base64url>`) MUST have
a valid id and signature; exactly one `t` and one `expiration`; at least one
well-formed `x` and one `server`, whose sets include the current blob and
the current node's name; MUST NOT be materially in the future; MUST have a
creation-to-expiry lifetime of at most five minutes; MUST NOT carry
duplicate singleton tags; and its operation MUST match the endpoint. A node
with an empty owner set and no grants is read-only. Public writes are an
explicit operator opt-in and never the default.

## 5. Serving

`GET` and `HEAD` by hash. A single RFC 7233 byte range MUST be
honoured exactly (`206`, `Content-Range`, `Accept-Ranges: bytes`); a shell
MAY rely on it for sampled verification. A blob is served with a declared
content type only when an `owner` claim supplies a safe one and no owner
declaration conflicts; otherwise it is served as
`application/octet-stream` with `Content-Disposition: attachment` and
`X-Content-Type-Options: nosniff`. A transport never chooses or promotes a
retention class.

`GET /list/<pubkey>` is scoped by who asks. A requester whose BUD-11
`list` event is signed by a key in the owner set MAY list any key in that
owner set (node-to-node reconcile of a keeper's own blobs). A requester
holding a valid friend grant MAY list only the blobs under that grant's own
claims -- never the owner's whole catalogue, which would expose the keeper's
kith and commons relationships. Anonymous listing is self-only, as 0.1.1.
Descriptors carry `class`.

## 6. Deletion and tombstones

BUD-12 `delete` removes the signer's own claim; bytes go when the last claim
goes. A tombstone `{ hash, signer_pubkey, created_at }` written by
the shell MUST cause the core to remove the blob and refuse the hash at
`PUT /mirror` and at `PUT /upload` from any signer who is not an owner.
An `owner` upload of a tombstoned hash MUST succeed and
clears the tombstone, so a mistaken takedown is recoverable by the keeper.
Writing a tombstone is a shell operation; honouring another node's
tombstone is shell policy.

## 7. Mirror and repair

The router accepts only hash-addressed sources allowed by its source policy
and obtains bytes through `BlobFetcher`, which returns a bounded stream,
declared size, declared media type and the `FetchPath` actually used. The
core independently enforces exact size and digest before commit. A source
becomes a repair candidate only after serving verified bytes; repair
repeats the same checks. Each repair source carries
`verified_at`, updated on every verified fetch from it; each blob carries
`last_verified`, updated by the integrity scan. A `FetchPath` of
`Direct`, `Relayed`, `Tor` or `Https` is recorded as evidence of the path
used and changes nothing about authority, retention or custody. A recorded
source is not a custody promise.

## 8. Storage above transport

The store's API is the Blossom router, exposed unbound. A transport is a
listener or a `BlobFetcher` and nothing more. The lane MUST NOT learn
`class`, retention tier, claims or the owner set. A transport success or
failure MUST NOT change a row; only §2 does. Relay-only and Tor-only are
valid configurations, not degraded states.

## 9. Outside this half

The encryption envelope (a separate joint specification: Wildbloom's
`aes-256-gcm-chunked-v1` construction, key source outside the frame,
neutral magic and MIME names, three known-answer vectors, and the review rule recorded in section E); node claim / status events and their kinds; peer
discovery; replica promises and placement; proof of storage; paid quota;
the transport half.

## 10. Versioning and acceptance

As 0.1.1: Rust API, schema, validation and observable HTTP behaviour are
versioned together; compatible schema changes migrate forward without
losing blobs or claims; weakening validation, changing retention meaning or
a destructive migration is a major version needing review by both owners.
Nothing is described as shipped until its own test has run on the named
platform or between independent nodes and is recorded by date.

---

# Transport half

*The transport half of the joint Wildbloom / Bothy core contract. It defines
how bytes move between nodes without the storage core learning which lane
carried them. It sits under the `BlobFetcher` interface the storage half owns,
and it is deliberately replaceable: Tor and ordinary HTTPS remain independent
routes, and the ForgeSworn Link lane is an optimisation, never a requirement.
Normative words: MUST / MUST NOT / MAY. The design and its known-answer vectors
are frozen in `forgesworn-link` (`link-core`, `vectors/`) at v0.1.0. Nothing is
final until both owners sign.*

## T1. Node transport identity

Every node MUST mint a persistent transport key on first run: Ed25519. It is
node identity only, distinct from the owner's Nostr key, and carries no
authority over any blob. It MUST be stored in platform-protected state with
restrictive permissions. A stolen transport key is a stolen node identity, not
a stolen owner identity.

The node ID is the 32-byte Ed25519 public key. Its canonical text form MUST be
lower-case RFC 4648 base32 without padding: 52 characters, which fits a DNS
label so a bridge host can carry it.

The single TLS rule, enforced on both sides of every connection: an endpoint
presents a self-signed leaf whose SubjectPublicKeyInfo is exactly its Ed25519
node key. Verification is one rule and one rule only -- the presented SPKI MUST
byte-equal the expected node ID's SPKI, and the TLS 1.3 CertificateVerify MUST
validate under that key. Chain, names and validity dates are ignored; card
expiry governs freshness. An endpoint presenting any other key MUST fail closed.

Rotation: a new transport key is a new node ID, a new card and a re-pairing. It
MUST NOT change any stored blob claim, which belongs to a Nostr signer, not a
transport key.

## T2. Address card, FSL-CARD-1

A node's reachability travels as a signed, short-lived binary card. It carries
the node ID, an issued-at and an expires-at (at most seven days after issue), a
per-node strictly increasing serial, and hints: relay `wss://` URLs; UDP
candidates, included only with the owner's consent; and an optional Tor onion
pointer. The card is signed by the transport key over a domain-prefixed body.

Verification MUST fail closed and in order: a card is rejected if it is
malformed, wrong-versioned, unsigned by its node ID, future-dated beyond a small
skew, expired, over-long-lived, carries too many hints, replays or lowers the
serial last seen for that node, or is not the node ID the verifier was told to
expect. There is no partial acceptance, and a verifier never accepts a card it
cannot fully parse.

The exact byte format and the known-answer vectors are frozen in
`forgesworn-link` (`link-core` and `vectors/`); this contract pins `FSL-CARD-1`
by those vectors. A node MUST persist the highest serial it has issued, so that
a backward clock step cannot mint a fresh card that fails another node's
replay check on the serial. The card is what a node's status event carries; publishing it
is the shell's, per the storage half §9.

## T3. Session interface

The transport exposes a narrow surface and nothing wider:

- `connect(card) -> Session`, `accept() -> Session`;
- `open_stream() -> Stream`, `accept_stream() -> Stream`;
- `close()`;
- `path() -> PathReport` -- status, relay in use, proved direct address, the
  cause of the last transition, and since when;
- `history() -> [PathTransition]` -- the bounded, oldest-first record of path
  transitions.

A `Session` is one QUIC connection to one authenticated peer; a `Stream` is one
bidirectional byte stream carrying `AsyncRead + AsyncWrite`. There is no framing
above QUIC and no knowledge of Blossom in this surface. `path()` MUST report the
path actually in use, taken from proof, never inferred from a successful
transfer. The recorded transition history is part of the interface, not only of
the log: a change too brief to catch by sampling `path()` (a sub-second
reconnect) MUST still appear in `history()`.

## T4. Connection state machine

Every node MUST be able to make outbound connections from an ordinary home
firewall or carrier network:

1. connect outbound to one or more configured relays;
2. authenticate the endpoint and register only a short-lived routing token;
3. exchange observed UDP candidates only after the peer is authenticated and
   only when the owner has consented to direct paths;
4. attempt a simultaneous direct QUIC path without changing application state;
5. upgrade to the direct path only after both endpoints prove the expected
   transport identity;
6. on direct failure, carry the same session over the relay;
7. on relay loss, reconnect to the next configured relay, then the same one;
8. if the lane fails entirely, standard Blossom over Tor or HTTPS remains a
   separately configured route.

Entering or leaving the direct path MUST NOT stall, duplicate or corrupt an open
stream. A terminal failure MUST be reported once, with a cause; there is no
half-open state that reads as success.

## T5. Relay-knowledge boundary

A relay MAY learn: endpoint routing tokens, source addresses, timing and byte
counts. A relay MUST NOT learn: Nostr keys; application authorisation events;
blob plaintext or envelope keys; retention tier or `class`; content hashes,
unless the application deliberately exposes them; or the long-term identity
behind a transport token. Relay frames are bounded, authenticated, rate-limited
and opaque; the payload is a QUIC packet protected by the TLS 1.3 keys of the
two endpoints, so a relay that reads every frame reads no plaintext. Operators
MAY run several relays from different organisations, and a client MAY remove
every ForgeSworn-operated relay and still work. No ForgeSworn hostname is a
mandatory default in the wire format; a founders' relay list is configuration
with a stated sunset, not protocol.

## T6. The `fsl` scheme and the fetcher

A Link mirror or repair source is a URL `fsl://<52-char-base32-node-id>/<sha256>`
with an optional extension. A `BlobFetcher` for this lane MUST reject any
non-`fsl` scheme with `UnsupportedSource`, so a multi-lane node dispatches by
scheme: `https` to the HTTPS or Tor fetcher, `fsl` to Link. The fetcher resolves
the node ID to a current `FSL-CARD-1` through a shell-provided `CardResolver`,
fed from the peer's status event; the lane itself does not touch Nostr. It then
connects, opens one stream, and streams the hash-addressed blob. The core
independently enforces exact length and SHA-256 before commit, per the storage
half §7; the lane never re-hashes on the core's behalf and never buffers a whole
blob.

The blob-request wire (`link-blossom`): a request is the magic `FSLB`, a version
byte and the 32-byte SHA-256; a response is a status byte, then for success the
`u64` size, the media type, and exactly that many body bytes streamed in chunks
of at most 64 KiB. A short or over-long stream MUST be treated as truncation and
refused. A `Session` to a node MAY be reused across fetches to that node; the
contract does not require a QUIC handshake per blob, so a scrub over many blobs
is not many handshakes.

## T7. FetchPath is evidence, not authority

A `FetchPath` of `Direct` or `Relayed` -- and `Tor` or `Https` for the other
lanes -- is recorded as evidence of the path actually used and changes nothing
about authority, retention or custody. This restates the storage half §7 from
the transport side.

## T8. Tor and non-Tor, both, by design

The lane is one path among several, and the node owner chooses. Tor MUST remain
an independent, separately configured route; ordinary HTTPS MUST remain a valid
route; the Link lane is an optimisation, never a requirement. Relay-only and
Tor-only are valid configurations, not degraded states. A node reachable only
over Tor, only over operator-managed HTTPS, or over the Link lane is a
conformant node. A browser or stock client that will not run Tor reaches a node
over standard Blossom HTTPS, directly or through a ciphertext-only bridge that
opens a Link session to the named node. Some operators will choose Tor for its
privacy and accept its reach cost; others will not; the contract supports both
and privileges neither.

## T9. Gates before the lane enters a product's shipped path

Before the Link lane becomes a default in a product's shipped path -- Bothy
Phase 1, or Wildbloom node repair -- the following MUST be measured on the shared
devices and networks and recorded by date: CGNAT-to-CGNAT and double-hostile-NAT
direct attempts, recording direct, relayed and failed separately; Android
24-hour background survival on a real carrier; binary size and battery; a
relay-side capture proving opacity; and one exact BUD-04 mirror and repair
through each successful path. If a gate fails for a pair, the answer for that
pair is relay-only or Tor, and the product ships with the HTTPS bridge and Tor,
which already work. Two known Link 0.2 items are preconditions for the network
cases they name: per-peer relay selection from a peer's card hints, before
multi-node federation; and re-announce-on-interface-change, for a phone moving
from Wi-Fi to mobile, until which such a phone is relay-backed, not broken.

The real-network evidence gathered so far -- direct upgrade to a public host,
direct across a LAN, a 258 ms mid-transfer relay failover, and a UDP-blocked
host falling back to the relay, all with hashes verified -- is recorded by date
in `forgesworn-link/acceptance/2026-08-27-real-network.md` and is cited by that
date. It does not yet cover the CGNAT or Android gates above.

## T10. Outside the transport half

Peer discovery; the node claim and status event kinds and their `nip-drafts`
work (shell); the encryption envelope; replica placement and promises; proof of
storage. The `link-relay` binary and its bounds, and the Link protocol, spec and
vectors, are `forgesworn-link` (published v0.1.0, MIT). The Android wrapper
around the Link `Endpoint` and the founders' relay deployment configuration are
Bothy's to build, reviewed in detail on the transport side before production.

---

# Signatures

Each owner signs by committing their name and the date to this file from their
own identity. A signature means: I have read both halves and §E, and I bind my
product to this contract at this version.

- Wildbloom / ForgeSworn Link owner: __________________________  date: __________
- Bothy owner: __________________________  date: __________

# E. Owner item -- the envelope's independent review

The joint encryption-envelope specification (a separate document,
`ENVELOPE.md`) calls for independent cryptographic review. This is an owner
decision on cost and timing, put to both owners as one question: who funds the
review, and does it gate the first shipped release or run in parallel.

- **Bothy side, decided by its owner 2026-08-28:** the envelope is reviewed
  internally using several models different from the ones that wrote it, to
  reach beta; it is open source at that point so anyone may run their own
  external review; no paid external audit, and it gates nothing. Honest wording
  for both products: *internally reviewed with independent models; open source
  for external review; not professionally audited.* Findings recorded by date
  beside `ENVELOPE.md`.
- **Wildbloom / ForgeSworn side:** _pending; to be recorded here in the
  owner's words. If it matches, this item closes; if it differs, the two owners
  settle it between them._

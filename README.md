# Shelter Kit

Shelter Kit is the shared Rust core for applications that need a secure,
content-addressed Blossom store without coupling storage policy to one network
listener or product UI.

It is deliberately a library, not a daemon.  A product supplies the listener,
transport and user experience; Shelter Kit supplies the unbound Axum router,
BUD authorisation, streaming content-addressed store, retention policy,
transport-neutral fetch interface and verified mirror/repair path.

Wildbloom and Bothy remain separate products because publishing and private
shelter have different consent and failure modes.  They can use the same core:
a Wildbloom Node is an owner-first keeper using Tor or direct HTTPS, while Bothy
can add its own pairing, federation and native transport around the same store
and router.

## Implemented in 0.1.1

- BUD-01, BUD-02, BUD-04, BUD-06, BUD-11 and self-only BUD-12 routing.
- Streaming reserve, hash verification and atomic content-addressed storage.
- One deduplicated store with owner, friend and best-effort guest claims.
- Fixed watermarks, expiring friend grants and oldest-guest-first eviction.
- Opaque serving for friend-only and guest-only blobs.
- A `BlobFetcher` boundary that keeps transport separate from authorisation,
  retention, hashing and storage.
- Direct public-HTTPS and loopback-proxied Tor fetch adapters, both with
  redirects disabled.  Direct DNS refuses non-public address space.
- Verified mirror sources and exact-length, exact-hash repair.
- SQLite schema migration and interrupted-transaction reconciliation.

## Added in 0.2.0

- Runtime owner set and friend grants: `AppState::set_owner_keys` and
  `AppState::set_friend_grants`, applied at a reconcile boundary.
- `FriendGrant::issuer`, so a shell can revoke every grant from one issuer.
- A nullable `class` on every claim and reservation, returned in list
  descriptors.  Schema `user_version = 3`.

A minor bump, not a patch: the API is additive but the capability is new, and
`FriendGrant` gained a field.

The exact compatibility promise is in [CORE-CONTRACT.md](CORE-CONTRACT.md); the
joint 0.2 contract this implements is in
[CORE-CONTRACT-0.2.md](CORE-CONTRACT-0.2.md).

## Not implemented here

Shelter Kit does not open a port, run Tor, punch through NAT, provide STUN/TURN,
encrypt application data, publish Nostr identity/status events, discover peers,
promise a replica count or prove durable custody.  Those are shell, application
or future protocol responsibilities.  Transport success is evidence that bytes
moved, not evidence that another machine will retain them.

## Use

```toml
[dependencies]
shelter-kit = { git = "https://github.com/forgesworn/shelter-kit", tag = "v0.1.1" }
```

Create a `Store`, build an `AppState` from a `BlossomConfig`, optionally supply
a `BlobFetcher`, then mount `shelter_kit::router(state)` into the product's
listener.  The config carries the shell's public product name and HTTPS source
URL, so shared storage does not blur the Wildbloom/Bothy front doors.  Tor and
direct public HTTPS are supplied fetch adapters; an application may provide
another adapter without moving trust decisions into that transport.

## Runtime policy

The owner set and the friend grants start in `BlossomConfig` but are not fixed
there.  A shell may re-supply either at any time with
`AppState::set_owner_keys(keys)` or `AppState::set_friend_grants(grants)`, and
`AppState::reconcile_now()` crosses the same boundary without changing the
policy.  Each call validates the whole resulting policy exactly as construction
does, so a refusal leaves the running node untouched, and each then waits for a
reconcile boundary: every write permit is acquired, so no upload or mirror is in
flight and none can start, the policy is swapped, and claims that have lost
their owner key or their grant are demoted to `guest` before the permits are
released.  A key cannot be an owner and hold a friend grant at the same time, so
promoting a friend to keeper is two ordered calls.  An in-flight upload always
commits under the policy it started with; a grant carries an optional `issuer`,
so revoking everything one issuer handed out is a single
`set_friend_grants` call with those grants left out.

## BUD-11 server tags

A configured accepted server name, and the `server` tag on an authorising
event, may each be either a bare lowercase domain (`node.example`) or an
absolute `http`/`https` URL with a host and optional port and nothing else
of significance -- a query, a fragment or userinfo makes the tag malformed.
Both shapes are accepted because real BUD-01 clients mint the latter:
`rust-nostr`'s `TagStandard::Server` carries a full `url::Url`, so a
compliant signer's token would otherwise be rejected by a node configured
with a bare name, or vice versa. A bare name matches a tag on the same host
under any scheme or port; two URLs must match exactly, scheme, host and
port alike, with the scheme's default port elided from both sides first.

## Claim class

A claim carries a nullable `class`, taken from a single `class` tag on the
authorising event, and `null` means "the shell's default".  It is the shell's
own label -- Bothy's `working`, `circle`, `vital`, `open` -- and the core
neither reads nor acts on it: it changes no retention tier, no eviction
decision and nothing about serving, and no response header mentions it.  It is
recorded, and it comes back in the `/list/<pubkey>` descriptors so the shell can
read its own label off the store.

The bound is 1 to 32 bytes of `[a-z0-9-]`.  A value outside it, a second `class`
tag, or a malformed one, is absent rather than an error: the tag carries no
authority, so refusing an otherwise valid upload over a label the core does not
act on would fail a good write for nothing.  A shell that cares reads the `null`
back in the descriptor.

## Status

This is a production candidate extracted from the reviewed Wildbloom Node core.
It has adversarial local tests and CI for macOS, Linux and Windows, but has not
received an independent security audit.  Do not treat it as a backup or custody
promise without independent replicas, monitoring and recovery evidence.

MIT licensed.  Sponsorship links are in [.github/FUNDING.yml](.github/FUNDING.yml).

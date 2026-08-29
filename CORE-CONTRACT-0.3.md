# Joint core contract -- 0.3

*A minimal bump on [`CORE-CONTRACT-0.2.md`](./CORE-CONTRACT-0.2.md). The whole of
0.2 -- both halves, §E and the boundary -- carries forward unchanged. 0.3 adds
one thing: it pins the application-layer encryption envelope to a named,
versioned specification, and records that 0.2's §E review item is discharged by
that specification's independent review. It is owned jointly: a change needs
both owners.*

## What 0.3 adds over 0.2

The shared encryption envelope is specified as **FSWNENC2**, in
[`FSWNENC2.md`](https://github.com/forgesworn/wildbloom/blob/7b3ae7f/docs/FSWNENC2.md),
superseding the implicit `FSWNENC1` construction both products had been using.

FSWNENC2 exists to close one exposure the review found: `FSWNENC1` is nonce-safe
only under a fresh key per envelope, so sealing many envelopes under one reused
key (a vault key) risks AES-GCM nonce reuse after a birthday-bounded number of
envelopes. FSWNENC2 carries a 32-byte random salt in the clear header and
derives the AES key per envelope with `HKDF-SHA256(input_key, salt,
"forgesworn-aes-256-gcm-chunked/v2")`, so per-file and vault sealing are
byte-identical except for the input key, which never appears in the envelope,
and the counter-from-zero nonce is safe regardless of key management. The
construction is byte-for-byte interoperable across both implementations, proven
by the known-answer vectors each side reproduces through its own seal path.

Both products adopt it in two staged steps, so no envelope is ever written that
the other side cannot read:

1. **Readers first (non-breaking).** Each side ships FSWNENC2 read support
   alongside its existing `FSWNENC1` and `WBLMENC1` readers. Shippable at any
   time.
2. **Writer default (breaking, coordinated).** Each side flips its encoder
   default to FSWNENC2 only after both read paths are deployed, in a window the
   owners pick together.

The media type is unchanged: `application/vnd.forgesworn.encrypted`, with the
legacy `application/vnd.wildbloom.encrypted` still accepted on read. The flip
forces no migration: existing `FSWNENC1` and `WBLMENC1` envelopes are read in
place and never re-sealed, the shared-key exposure is frozen at the flip, and
re-sealing is an opt-in step later.

## §E of 0.2 discharged

0.2 §E asked for an independent review of the envelope. That review is done and
recorded in
[`ENVELOPE-REVIEW.md`](https://github.com/forgesworn/wildbloom/blob/7b3ae7f/docs/ENVELOPE-REVIEW.md):
internally reviewed with independent models, open source for external review,
not professionally audited, gating nothing -- the wording both owners agreed in
0.2 §E. FSWNENC2 is the review's principal remediation; the remaining review
items are recorded there.

# Signatures

Each owner signs by committing their name and the date to this file from their
own identity. A signature means: I have read 0.2, this 0.3 bump and
`FSWNENC2.md`, and I bind my product to the contract at this version.

- Wildbloom / ForgeSworn Link owner:  date:
- Bothy owner: **decented**  date: **2026-08-29**

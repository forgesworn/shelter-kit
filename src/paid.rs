//! Operator-owned storage commitments. No payment credentials, wallets or network I/O.
//!
//! Only a trusted selling shell may reserve sales and activate them after verifying
//! settlement. These methods are not payment verification or a public checkout API.
use crate::store::{ClaimSpec, RetentionTier, Store, StoreError, unix_time};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;

/// Immutable storage portion of an operator quote. IDs are opaque local references,
/// never bearer instruments. A shell must durably bind its price and terms to this ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidSale {
    pub order_id: String,
    pub signer_pubkey: String,
    pub capacity_bytes: u64,
    pub duration_seconds: u64,
    pub grace_seconds: u64,
    pub hold_expires_at: u64,
    /// Renew a stable allowance without transferring its signer or changing its ceiling.
    pub renews: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaidAllowance {
    pub allowance_id: String,
    pub signer_pubkey: String,
    pub capacity_bytes: u64,
    pub starts_at: u64,
    pub writes_until: u64,
    pub retains_until: u64,
}

pub(crate) fn initialise(db: &Connection) -> Result<(), StoreError> {
    db.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS paid_allowances (
            id TEXT PRIMARY KEY NOT NULL, signer TEXT UNIQUE NOT NULL,
            capacity INTEGER NOT NULL CHECK(capacity > 0),
            starts INTEGER NOT NULL, ends INTEGER NOT NULL, retains INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS paid_sales (
            id TEXT PRIMARY KEY NOT NULL, signer TEXT NOT NULL,
            capacity INTEGER NOT NULL CHECK(capacity > 0),
            duration INTEGER NOT NULL CHECK(duration > 0), grace INTEGER NOT NULL CHECK(grace >= 0),
            expires INTEGER NOT NULL, renews TEXT REFERENCES paid_allowances(id),
            activated INTEGER, result_starts INTEGER, result_ends INTEGER, result_retains INTEGER
         );
         CREATE INDEX IF NOT EXISTS paid_sales_live ON paid_sales(activated, expires);
         COMMIT;",
    )?;
    Ok(())
}

fn as_i64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::IntegerRange)
}
fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn row_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
fn read_allowance(db: &Connection, id: &str) -> Result<Option<PaidAllowance>, StoreError> {
    Ok(db
        .query_row(
            "SELECT id, signer, capacity, starts, ends, retains FROM paid_allowances WHERE id = ?1",
            [id],
            |r| {
                Ok(PaidAllowance {
                    allowance_id: r.get(0)?,
                    signer_pubkey: r.get(1)?,
                    capacity_bytes: row_u64(r, 2)?,
                    starts_at: row_u64(r, 3)?,
                    writes_until: row_u64(r, 4)?,
                    retains_until: row_u64(r, 5)?,
                })
            },
        )
        .optional()?)
}
fn read_sale(db: &Connection, id: &str) -> Result<Option<PaidSale>, StoreError> {
    Ok(db.query_row(
        "SELECT id, signer, capacity, duration, grace, expires, renews FROM paid_sales WHERE id = ?1",
        [id], |r| Ok(PaidSale { order_id: r.get(0)?, signer_pubkey: r.get(1)?, capacity_bytes: row_u64(r, 2)?,
            duration_seconds: row_u64(r, 3)?, grace_seconds: row_u64(r, 4)?, hold_expires_at: row_u64(r, 5)?, renews: r.get(6)? }),
    ).optional()?)
}

impl Store {
    /// Reserve capacity before the selling shell offers payment. A replay of the exact
    /// live record is harmless; expired holds are refused. Holds last at most a day.
    /// New sales conservatively require free space; they do not evict guest blobs.
    pub fn reserve_paid_sale(&self, sale: &PaidSale) -> Result<(), StoreError> {
        if !valid_id(&sale.order_id)
            || sale.renews.as_deref().is_some_and(|id| !valid_id(id))
            || sale.signer_pubkey.len() != 64
            || !sale
                .signer_pubkey
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || sale.capacity_bytes == 0
            || sale.duration_seconds == 0
        {
            return Err(StoreError::InvalidPaidSale);
        }
        for n in [
            sale.capacity_bytes,
            sale.duration_seconds,
            sale.grace_seconds,
            sale.hold_expires_at,
        ] {
            as_i64(n)?;
        }
        // Also bound the combined term before any quote can be offered.
        as_i64(
            sale.hold_expires_at
                .checked_add(sale.duration_seconds)
                .and_then(|n| n.checked_add(sale.grace_seconds))
                .ok_or(StoreError::IntegerRange)?,
        )?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = unix_time()?;
        if let Some(existing) = read_sale(&tx, &sale.order_id)? {
            if existing != *sale {
                return Err(StoreError::InvalidPaidSale);
            }
            return if as_i64(existing.hold_expires_at)? > now {
                Ok(())
            } else {
                Err(StoreError::PaidAllowanceUnavailable)
            };
        }
        if as_i64(sale.hold_expires_at)? <= now || as_i64(sale.hold_expires_at)? > now + 86_400 {
            return Err(StoreError::InvalidPaidSale);
        }
        if let Some(id) = &sale.renews {
            let a = read_allowance(&tx, id)?.ok_or(StoreError::InvalidPaidSale)?;
            if a.signer_pubkey != sale.signer_pubkey || a.capacity_bytes != sale.capacity_bytes {
                return Err(StoreError::InvalidPaidSale);
            }
            as_i64(
                a.writes_until
                    .max(now as u64)
                    .checked_add(sale.duration_seconds)
                    .and_then(|n| n.checked_add(sale.grace_seconds))
                    .ok_or(StoreError::IntegerRange)?,
            )?;
        } else if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM paid_allowances WHERE signer = ?1)",
            [&sale.signer_pubkey],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(StoreError::InvalidPaidSale);
        }
        // One pending sale per signer prevents ambiguous or overlapping activation.
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM paid_sales WHERE signer = ?1 AND activated IS NULL AND expires > ?2)",
            params![sale.signer_pubkey, now], |r| r.get::<_, bool>(0))? {
            return Err(StoreError::InvalidPaidSale);
        }
        tx.execute("INSERT INTO paid_sales (id, signer, capacity, duration, grace, expires, renews) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![sale.order_id, sale.signer_pubkey, as_i64(sale.capacity_bytes)?, as_i64(sale.duration_seconds)?,
                as_i64(sale.grace_seconds)?, as_i64(sale.hold_expires_at)?, sale.renews])?;
        if committed_usage(&tx, now)? > as_i64(self.quota_bytes())? {
            return Err(StoreError::QuotaExceeded);
        }
        tx.commit()?;
        Ok(())
    }

    /// Activate only after authoritative settlement. Returns the original activation
    /// result on replay, even after later renewals. Expired holds are refused: the shell
    /// must fulfil through a new reserved sale or handle the seller's refund obligation.
    pub fn activate_paid_sale(&self, order_id: &str) -> Result<PaidAllowance, StoreError> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = unix_time()?;
        let sale = read_sale(&tx, order_id)?.ok_or(StoreError::InvalidPaidSale)?;
        let prior = tx.query_row("SELECT result_starts, result_ends, result_retains FROM paid_sales WHERE id = ?1 AND activated IS NOT NULL", [order_id],
            |r| Ok((row_u64(r, 0)?, row_u64(r, 1)?, row_u64(r, 2)?))).optional()?;
        let id = sale.renews.clone().unwrap_or_else(|| sale.order_id.clone());
        if let Some((starts_at, writes_until, retains_until)) = prior {
            return Ok(PaidAllowance {
                allowance_id: id,
                signer_pubkey: sale.signer_pubkey,
                capacity_bytes: sale.capacity_bytes,
                starts_at,
                writes_until,
                retains_until,
            });
        }
        if as_i64(sale.hold_expires_at)? <= now {
            return Err(StoreError::PaidAllowanceUnavailable);
        }
        let old = read_allowance(&tx, &id)?;
        let starts_at = old.as_ref().map_or(now as u64, |a| a.starts_at);
        let base = old
            .as_ref()
            .map_or(now as u64, |a| a.writes_until.max(now as u64));
        let writes_until = base
            .checked_add(sale.duration_seconds)
            .ok_or(StoreError::IntegerRange)?;
        let retains_until = writes_until
            .checked_add(sale.grace_seconds)
            .ok_or(StoreError::IntegerRange)?
            .max(old.as_ref().map_or(0, |a| a.retains_until));
        as_i64(retains_until)?;
        tx.execute("INSERT INTO paid_allowances (id,signer,capacity,starts,ends,retains) VALUES (?1,?2,?3,?4,?5,?6)
            ON CONFLICT(id) DO UPDATE SET ends=excluded.ends, retains=excluded.retains",
            params![id, sale.signer_pubkey, as_i64(sale.capacity_bytes)?, as_i64(starts_at)?, as_i64(writes_until)?, as_i64(retains_until)?])?;
        tx.execute("UPDATE claims SET claim_expires_at = ?1 WHERE retention_tier = 'paid' AND grant_id = ?2", params![as_i64(retains_until)?, id])?;
        tx.execute("UPDATE paid_sales SET activated=?1, result_starts=?2, result_ends=?3, result_retains=?4 WHERE id=?5",
            params![now, as_i64(starts_at)?, as_i64(writes_until)?, as_i64(retains_until)?, order_id])?;
        if committed_usage(&tx, now)? > as_i64(self.quota_bytes())? {
            return Err(StoreError::QuotaExceeded);
        }
        tx.commit()?;
        Ok(PaidAllowance {
            allowance_id: id,
            signer_pubkey: sale.signer_pubkey,
            capacity_bytes: sale.capacity_bytes,
            starts_at,
            writes_until,
            retains_until,
        })
    }

    pub fn paid_allowance(&self, id: &str) -> Result<Option<PaidAllowance>, StoreError> {
        read_allowance(&self.connection()?, id)
    }

    /// Resolve a previously authenticated signer. This does not authenticate a request.
    pub fn paid_claim(
        &self,
        signer: &str,
        declared_type: &str,
        class: Option<String>,
    ) -> Result<Option<ClaimSpec>, StoreError> {
        let db = self.connection()?;
        let id = db
            .query_row(
                "SELECT id FROM paid_allowances WHERE signer=?1 AND ends > ?2",
                params![signer, unix_time()?],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(id.map(|id| ClaimSpec {
            signer_pubkey: signer.to_owned(),
            retention_tier: RetentionTier::Paid,
            declared_type: declared_type.to_owned(),
            grant_id: Some(id),
            claim_expires_at: None,
            byte_limit: None,
            class,
        }))
    }

    /// Physical bytes, in-flight writes, unsatisfied paid capacity and live sale holds.
    /// Separate from physical storage statistics; deduplication cannot sell one user's allowance twice.
    pub fn committed_bytes(&self) -> Result<u64, StoreError> {
        Ok(committed_usage(&self.connection()?, unix_time()?)? as u64)
    }
}

fn active(db: &Connection, claim: &ClaimSpec, now: i64) -> Result<PaidAllowance, StoreError> {
    let a = read_allowance(
        db,
        claim
            .grant_id
            .as_deref()
            .ok_or(StoreError::PaidAllowanceUnavailable)?,
    )?
    .ok_or(StoreError::PaidAllowanceUnavailable)?;
    if a.signer_pubkey != claim.signer_pubkey || as_i64(a.writes_until)? <= now {
        return Err(StoreError::PaidAllowanceUnavailable);
    }
    Ok(a)
}
pub(crate) fn retention_end(
    db: &Connection,
    claim: &ClaimSpec,
    now: i64,
) -> Result<i64, StoreError> {
    as_i64(active(db, claim, now)?.retains_until)
}
fn logical_usage(db: &Connection, id: &str) -> Result<i64, StoreError> {
    Ok(db.query_row("SELECT
        COALESCE((SELECT SUM(b.size) FROM claims c JOIN blobs b ON b.hash=c.hash WHERE c.retention_tier='paid' AND c.grant_id=?1),0)
        + COALESCE((SELECT SUM(size) FROM reservations WHERE retention_tier='paid' AND grant_id=?1),0)", [id], |r| r.get(0))?)
}
pub(crate) fn write_credit(
    db: &Connection,
    claim: &ClaimSpec,
    hash: &str,
    size: u64,
    now: i64,
) -> Result<i64, StoreError> {
    if claim.retention_tier != RetentionTier::Paid {
        return Ok(0);
    }
    let a = active(db, claim, now)?;
    let claimed = db.query_row("SELECT EXISTS(SELECT 1 FROM claims WHERE hash=?1 AND signer_pubkey=?2 AND retention_tier='paid' AND grant_id=?3)",
        params![hash, claim.signer_pubkey, a.allowance_id], |r| r.get::<_, bool>(0))?;
    Ok(if claimed { 0 } else { as_i64(size)? })
}
pub(crate) fn enforce_limit(
    db: &Connection,
    claim: &ClaimSpec,
    hash: &str,
    size: u64,
    now: i64,
) -> Result<(), StoreError> {
    let a = active(db, claim, now)?;
    let used = logical_usage(db, &a.allowance_id)?;
    if used.saturating_add(write_credit(db, claim, hash, size, now)?) > as_i64(a.capacity_bytes)? {
        return Err(StoreError::PaidAllowanceUnavailable);
    }
    Ok(())
}

pub(crate) fn committed_usage(db: &Connection, now: i64) -> Result<i64, StoreError> {
    // Reserve each full sold ceiling. Only paid-only physical copies covered by
    // a live commitment are included in that ceiling. Shared non-paid copies
    // count separately so deleting a paid claim cannot oversell its unused space.
    // A renewal hold preserves the commitment across the old write deadline.
    Ok(db.query_row(
        "WITH live AS (
            SELECT id, capacity FROM paid_allowances a WHERE ends > ?1 OR EXISTS (
                SELECT 1 FROM paid_sales s WHERE s.renews=a.id AND s.activated IS NULL AND s.expires > ?1)
         )
         SELECT COALESCE((SELECT SUM(capacity) FROM live),0)
          + COALESCE((SELECT SUM(capacity) FROM paid_sales WHERE activated IS NULL AND expires > ?1 AND renews IS NULL),0)
          + COALESCE((SELECT SUM(b.size) FROM blobs b WHERE
                EXISTS (SELECT 1 FROM claims c WHERE c.hash=b.hash AND c.retention_tier != 'paid')
                OR NOT EXISTS (SELECT 1 FROM claims c JOIN live l ON l.id=c.grant_id WHERE c.hash=b.hash AND c.retention_tier='paid')),0)
          + COALESCE((SELECT SUM(r.size) FROM reservations r WHERE r.retention_tier != 'paid'
                OR NOT EXISTS (SELECT 1 FROM live l WHERE l.id=r.grant_id)),0)",
        [now], |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{StoreConfig, UploadStart};
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        sync::{Arc, Barrier},
    };

    fn store(quota: u64) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(StoreConfig {
            root: dir.path().to_owned(),
            quota_bytes: quota,
            max_blob_bytes: quota,
        })
        .unwrap();
        (dir, store)
    }
    fn sale(id: &str, signer: u8, capacity: u64) -> PaidSale {
        PaidSale {
            order_id: id.into(),
            signer_pubkey: format!("{signer:064x}"),
            capacity_bytes: capacity,
            duration_seconds: 3600,
            grace_seconds: 600,
            hold_expires_at: unix_time().unwrap() as u64 + 300,
            renews: None,
        }
    }
    fn buy(store: &Store, sale: &PaidSale) -> PaidAllowance {
        store.reserve_paid_sale(sale).unwrap();
        store.activate_paid_sale(&sale.order_id).unwrap()
    }
    fn claim(store: &Store, signer: u8) -> ClaimSpec {
        store
            .paid_claim(&format!("{signer:064x}"), "text/plain", None)
            .unwrap()
            .unwrap()
    }
    fn owner(signer: &str) -> ClaimSpec {
        ClaimSpec {
            signer_pubkey: signer.into(),
            retention_tier: RetentionTier::Owner,
            declared_type: "text/plain".into(),
            grant_id: None,
            claim_expires_at: None,
            byte_limit: None,
            class: None,
        }
    }
    fn put(store: &Store, bytes: &[u8], claim: ClaimSpec) -> String {
        let hash = hex::encode(Sha256::digest(bytes));
        if let UploadStart::Reserved(r) = store
            .begin_claimed_upload(&hash, bytes.len() as u64, claim)
            .unwrap()
        {
            fs::write(r.temp_path(), bytes).unwrap();
            r.commit(&hash, bytes.len() as u64).unwrap();
        }
        hash
    }
    fn expire_writes(store: &Store, id: &str) {
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE paid_allowances SET ends=?1 WHERE id=?2",
                params![unix_time().unwrap() - 1, id],
            )
            .unwrap();
    }
    #[test]
    fn holds_are_durable_bounded_immutable_and_block_owner_writes() {
        let (_dir, store) = store(100);
        let s = sale("order", 1, 80);
        store.reserve_paid_sale(&s).unwrap();
        store.reserve_paid_sale(&s).unwrap();
        let mut altered = s.clone();
        altered.capacity_bytes = 81;
        assert!(matches!(
            store.reserve_paid_sale(&altered),
            Err(StoreError::InvalidPaidSale)
        ));
        assert!(matches!(
            store.reserve_paid_sale(&sale("other", 2, 21)),
            Err(StoreError::QuotaExceeded)
        ));
        assert!(matches!(
            store.begin_claimed_upload(&"a".repeat(64), 21, owner("owner")),
            Err(StoreError::QuotaExceeded)
        ));
        assert!(matches!(
            store.set_quota_bytes(79),
            Err(StoreError::QuotaBelowUsage { .. })
        ));
        let config = store.config();
        drop(store);
        let store = Store::open(config).unwrap();
        assert_eq!(store.committed_bytes().unwrap(), 80);
        let a = store.activate_paid_sale("order").unwrap();
        assert_eq!(a, store.activate_paid_sale("order").unwrap());
        assert_eq!(a.writes_until - a.starts_at, 3600);
        assert_eq!(store.committed_bytes().unwrap(), 80);
    }
    #[test]
    fn concurrent_sales_cannot_oversell() {
        let (_dir, store) = store(100);
        let barrier = Arc::new(Barrier::new(2));
        let threads: Vec<_> = (1..=2)
            .map(|i| {
                let s = store.clone();
                let b = barrier.clone();
                std::thread::spawn(move || {
                    b.wait();
                    s.reserve_paid_sale(&sale(&format!("sale{i}"), i, 60))
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(store.committed_bytes().unwrap(), 60);
    }
    #[test]
    fn dedup_counts_each_signer_and_reservations_enforce_logical_ceiling() {
        let (_dir, store) = store(100);
        buy(&store, &sale("a", 1, 10));
        buy(&store, &sale("b", 2, 10));
        let hash = put(&store, b"shared", claim(&store, 1));
        put(&store, b"shared", claim(&store, 2));
        assert_eq!(store.stats().unwrap().bytes, 6);
        assert_eq!(store.committed_bytes().unwrap(), 20);
        let hold = store
            .begin_claimed_upload(&"a".repeat(64), 4, claim(&store, 1))
            .unwrap();
        assert!(matches!(
            store.begin_claimed_upload(&"b".repeat(64), 1, claim(&store, 1)),
            Err(StoreError::PaidAllowanceUnavailable)
        ));
        drop(hold);
        assert!(
            store
                .begin_claimed_upload(&"b".repeat(64), 4, claim(&store, 1))
                .is_ok()
        );
        store.delete_claim(&hash, &format!("{:064x}", 1)).unwrap();
        assert!(store.get(&hash).unwrap().is_some());
        assert_eq!(store.committed_bytes().unwrap(), 20);
    }
    #[test]
    fn full_pool_renewal_is_idempotent_and_does_not_demote_paid_claims() {
        let (_dir, store) = store(100);
        let original = buy(&store, &sale("a", 1, 100));
        let hash = put(&store, &[1; 100], claim(&store, 1));
        let mut renewal = sale("renew", 1, 100);
        renewal.renews = Some("a".into());
        store.reserve_paid_sale(&renewal).unwrap();
        let renewed = store.activate_paid_sale("renew").unwrap();
        assert_eq!(renewed.writes_until, original.writes_until + 3600);
        assert_eq!(store.activate_paid_sale("a").unwrap(), original);
        assert_eq!(store.activate_paid_sale("renew").unwrap(), renewed);
        store
            .reconcile_claim_policy(&Default::default(), &Default::default())
            .unwrap();
        assert_eq!(
            store.get(&hash).unwrap().unwrap().retention_tier,
            RetentionTier::Paid
        );
        assert!(matches!(
            store.begin_claimed_upload(&"a".repeat(64), 1, owner("owner")),
            Err(StoreError::QuotaExceeded)
        ));
        assert_eq!(store.evict_to_watermark().unwrap(), 0);
        let config = store.config();
        drop(store);
        let store = Store::open(config).unwrap();
        assert_eq!(store.paid_allowance("a").unwrap(), Some(renewed));
        assert!(store.get(&hash).unwrap().is_some());
    }
    #[test]
    fn grace_refuses_writes_and_expiry_preserves_other_claims() {
        let (_dir, store) = store(100);
        buy(&store, &sale("a", 1, 20));
        let saved = claim(&store, 1);
        let hash = put(&store, b"shared", saved.clone());
        put(&store, b"shared", owner("owner"));
        expire_writes(&store, "a");
        assert!(
            store
                .paid_claim(&saved.signer_pubkey, "text/plain", None)
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            store.begin_claimed_upload(&"a".repeat(64), 1, saved),
            Err(StoreError::PaidAllowanceUnavailable)
        ));
        assert_eq!(store.reap_expired_claims().unwrap(), 0);
        assert!(store.get(&hash).unwrap().is_some());
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE claims SET claim_expires_at=?1 WHERE retention_tier='paid'",
                [unix_time().unwrap() - 1],
            )
            .unwrap();
        store.reap_expired_claims().unwrap();
        assert_eq!(
            store.get(&hash).unwrap().unwrap().retention_tier,
            RetentionTier::Owner
        );
    }
    #[test]
    fn expired_hold_does_not_activate_or_refresh_on_replay() {
        let (_dir, store) = store(100);
        let s = sale("late", 1, 100);
        store.reserve_paid_sale(&s).unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE paid_sales SET expires=?1",
                [unix_time().unwrap() - 1],
            )
            .unwrap();
        assert_eq!(store.committed_bytes().unwrap(), 0);
        assert!(matches!(
            store.activate_paid_sale("late"),
            Err(StoreError::PaidAllowanceUnavailable)
        ));
        assert!(matches!(
            store.reserve_paid_sale(&s),
            Err(StoreError::InvalidPaidSale)
        ));
        assert!(store.paid_allowance("late").unwrap().is_none());
    }
    #[test]
    fn wrong_signer_and_stream_crossing_expiry_cannot_commit() {
        let (_dir, store) = store(100);
        buy(&store, &sale("a", 1, 100));
        let mut forged = claim(&store, 1);
        forged.signer_pubkey = format!("{:064x}", 2);
        assert!(matches!(
            store.begin_claimed_upload(&"a".repeat(64), 1, forged),
            Err(StoreError::PaidAllowanceUnavailable)
        ));
        let bytes = b"stream";
        let hash = hex::encode(Sha256::digest(bytes));
        let UploadStart::Reserved(r) = store
            .begin_claimed_upload(&hash, 6, claim(&store, 1))
            .unwrap()
        else {
            panic!()
        };
        fs::write(r.temp_path(), bytes).unwrap();
        expire_writes(&store, "a");
        assert!(matches!(
            r.commit(&hash, 6),
            Err(StoreError::PaidAllowanceUnavailable)
        ));
        assert_eq!(store.stats().unwrap().reserved_bytes, 0);
        assert!(store.get(&hash).unwrap().is_none());
    }
    #[test]
    fn shared_owner_bytes_stay_accounted_after_paid_deletion() {
        let (_dir, store) = store(100);
        buy(&store, &sale("a", 1, 80));
        let hash = put(&store, &[1; 20], claim(&store, 1));
        put(&store, &[1; 20], owner("owner"));
        assert_eq!(store.committed_bytes().unwrap(), 100);
        store.delete_claim(&hash, &format!("{:064x}", 1)).unwrap();
        assert_eq!(store.committed_bytes().unwrap(), 100);
        assert!(matches!(
            store.reserve_paid_sale(&sale("b", 2, 1)),
            Err(StoreError::QuotaExceeded)
        ));
    }
    #[test]
    fn concurrent_uploads_cannot_exceed_one_allowance() {
        let (_dir, store) = store(100);
        buy(&store, &sale("a", 1, 10));
        let barrier = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let threads: Vec<_> = (1..=2)
            .map(|i| {
                let store = store.clone();
                let barrier = barrier.clone();
                let release = release.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let result =
                        store.begin_claimed_upload(&format!("{i:064x}"), 6, claim(&store, 1));
                    release.wait();
                    result.is_ok()
                })
            })
            .collect();
        let passed = threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .filter(|ok| *ok)
            .count();
        assert_eq!(passed, 1);
        assert_eq!(store.stats().unwrap().reserved_bytes, 0);
    }

    #[test]
    fn quote_expiry_releases_capacity_and_new_sale_survives_restart() {
        let (_dir, store) = store(100);
        store.reserve_paid_sale(&sale("old", 1, 100)).unwrap();
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE paid_sales SET expires=?1",
                [unix_time().unwrap() - 1],
            )
            .unwrap();
        let replacement = sale("replacement", 1, 100);
        store.reserve_paid_sale(&replacement).unwrap();
        let config = store.config();
        drop(store);
        let store = Store::open(config).unwrap();
        assert!(matches!(
            store.activate_paid_sale("old"),
            Err(StoreError::PaidAllowanceUnavailable)
        ));
        assert_eq!(
            store
                .activate_paid_sale("replacement")
                .unwrap()
                .capacity_bytes,
            100
        );
    }

    #[test]
    fn grace_renewal_reserves_unused_space_and_renewal_hold_survives_deadline() {
        let (_dir, store) = store(100);
        buy(&store, &sale("a", 1, 80));
        put(&store, &[1; 20], claim(&store, 1));
        let mut renewal = sale("renew", 1, 80);
        renewal.renews = Some("a".into());
        store.reserve_paid_sale(&renewal).unwrap();
        expire_writes(&store, "a");
        assert_eq!(store.committed_bytes().unwrap(), 80);
        assert!(matches!(
            store.reserve_paid_sale(&sale("b", 2, 21)),
            Err(StoreError::QuotaExceeded)
        ));
        store.activate_paid_sale("renew").unwrap();
        assert!(
            store
                .paid_claim(&renewal.signer_pubkey, "text/plain", None)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn expired_only_copy_is_collected_and_live_copy_is_protected() {
        let (_dir, store) = store(100);
        buy(&store, &sale("a", 1, 40));
        buy(&store, &sale("b", 2, 40));
        let shared = put(&store, b"shared", claim(&store, 1));
        put(&store, b"shared", claim(&store, 2));
        let only = put(&store, b"only", claim(&store, 1));
        expire_writes(&store, "a");
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE claims SET claim_expires_at=?1 WHERE grant_id='a'",
                [unix_time().unwrap() - 1],
            )
            .unwrap();
        assert_eq!(store.reap_expired_claims().unwrap(), 1);
        assert!(store.get(&only).unwrap().is_none());
        assert!(store.get(&shared).unwrap().is_some());
        assert_eq!(
            store
                .list_claims(&format!("{:064x}", 2), None, 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn reopening_below_sold_capacity_fails_without_erasing_the_allowance() {
        let (_dir, store) = store(100);
        let original = buy(&store, &sale("a", 1, 80));
        let config = store.config();
        drop(store);
        let mut smaller = config.clone();
        smaller.quota_bytes = 79;
        assert!(matches!(
            Store::open(smaller),
            Err(StoreError::QuotaBelowUsage { .. })
        ));
        assert_eq!(
            Store::open(config).unwrap().paid_allowance("a").unwrap(),
            Some(original)
        );
    }
    #[test]
    fn adding_a_shared_owner_claim_cannot_spend_a_full_paid_commitment() {
        let (_dir, store) = store(100);
        buy(&store, &sale("a", 1, 100));
        let hash = put(&store, b"shared", claim(&store, 1));
        assert!(matches!(
            store.begin_claimed_upload(&hash, 6, owner("owner")),
            Err(StoreError::QuotaExceeded)
        ));
        assert!(store.list_claims("owner", None, 10).unwrap().is_empty());
        assert_eq!(
            store.get(&hash).unwrap().unwrap().retention_tier,
            RetentionTier::Paid
        );
        assert_eq!(store.stats().unwrap().committed_bytes, 100);
    }

    #[test]
    fn malformed_sales_and_conflicting_renewals_do_not_reserve_capacity() {
        let (_dir, store) = store(100);
        let mut bad = sale("a", 1, 100);
        bad.capacity_bytes = 0;
        assert!(matches!(
            store.reserve_paid_sale(&bad),
            Err(StoreError::InvalidPaidSale)
        ));
        bad = sale("a", 1, 100);
        bad.duration_seconds = u64::MAX;
        assert!(matches!(
            store.reserve_paid_sale(&bad),
            Err(StoreError::IntegerRange)
        ));
        bad = sale("a", 1, 100);
        bad.hold_expires_at += 86_400;
        assert!(matches!(
            store.reserve_paid_sale(&bad),
            Err(StoreError::InvalidPaidSale)
        ));
        assert_eq!(store.committed_bytes().unwrap(), 0);
        buy(&store, &sale("a", 1, 100));
        bad = sale("renew", 2, 100);
        bad.renews = Some("a".into());
        assert!(matches!(
            store.reserve_paid_sale(&bad),
            Err(StoreError::InvalidPaidSale)
        ));
        bad.signer_pubkey = format!("{:064x}", 1);
        bad.capacity_bytes = 99;
        assert!(matches!(
            store.reserve_paid_sale(&bad),
            Err(StoreError::InvalidPaidSale)
        ));
        assert_eq!(store.committed_bytes().unwrap(), 100);
    }

    #[test]
    fn schema_five_claim_constraint_migrates_without_losing_existing_bytes() {
        let (_dir, store) = store(100);
        let hash = put(&store, b"old owner", owner("owner"));
        store
            .connection()
            .unwrap()
            .execute_batch(
                "PRAGMA foreign_keys=OFF;
             BEGIN IMMEDIATE;
             CREATE TABLE old_claims (
                hash TEXT NOT NULL REFERENCES blobs(hash) ON DELETE CASCADE,
                signer_pubkey TEXT NOT NULL,
                retention_tier TEXT NOT NULL CHECK(retention_tier IN ('owner','friend','guest')),
                declared_type TEXT NOT NULL, grant_id TEXT, claim_expires_at INTEGER,
                created_at INTEGER NOT NULL, class TEXT, PRIMARY KEY(signer_pubkey,hash));
             INSERT INTO old_claims SELECT * FROM claims;
             DROP TABLE claims;
             ALTER TABLE old_claims RENAME TO claims;
             DROP TABLE paid_sales;
             DROP TABLE paid_allowances;
             PRAGMA user_version=5;
             COMMIT;",
            )
            .unwrap();
        let config = store.config();
        drop(store);
        let store = Store::open(config).unwrap();
        assert_eq!(fs::read(store.blob_path(&hash)).unwrap(), b"old owner");
        assert_eq!(
            store.get(&hash).unwrap().unwrap().retention_tier,
            RetentionTier::Owner
        );
        buy(&store, &sale("a", 1, 50));
        let paid = put(&store, b"new paid", claim(&store, 1));
        assert_eq!(
            store.get(&paid).unwrap().unwrap().retention_tier,
            RetentionTier::Paid
        );
    }
    #[test]
    fn same_signer_reupload_preserves_paid_retention_and_updates_advisory_class() {
        let (_dir, store) = store(100);
        buy(&store, &sale("a", 1, 100));
        let signer = format!("{:064x}", 1);
        let hash = put(&store, b"paid", claim(&store, 1));
        let mut promoted = owner(&signer);
        promoted.class = Some("vital".into());
        put(&store, b"paid", promoted);
        assert_eq!(
            store.get(&hash).unwrap().unwrap().retention_tier,
            RetentionTier::Paid
        );
        assert_eq!(
            store.list_claims(&signer, None, 10).unwrap()[0]
                .class
                .as_deref(),
            Some("vital")
        );
        assert_eq!(store.committed_bytes().unwrap(), 100);
    }
}

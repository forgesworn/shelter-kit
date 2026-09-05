//! Optional write posture. Passing a filter does not prove encryption.

use std::{fmt::Debug, sync::Arc};

pub const ADMISSION_PREFIX_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDecision {
    Accept,
    Reject,
}

/// Called with at most the first 4 KiB, after a write is reserved. A shorter
/// blob supplies its whole body. Implementations must be bounded and must not
/// perform I/O. No filter is the default.
pub trait AdmissionFilter: Debug + Send + Sync {
    fn inspect(&self, prefix: &[u8]) -> AdmissionDecision;
}

/// Reject common media signatures at offset zero and byte entropy below
/// 6 bits per byte. Empty and very short bodies also fail this heuristic.
/// Compressed plaintext can pass; valid encrypted data can fail. This is an
/// operator posture, never an encryption or content-safety guarantee.
#[derive(Debug)]
pub struct SealedParcelsOnly;

impl AdmissionFilter for SealedParcelsOnly {
    fn inspect(&self, prefix: &[u8]) -> AdmissionDecision {
        const MAGIC: &[&[u8]] = &[
            b"\x89PNG\r\n\x1a\n",
            b"\xff\xd8\xff",
            b"GIF87a",
            b"GIF89a",
            b"RIFF",
            b"OggS",
            b"fLaC",
            b"ID3",
            b"%PDF-",
        ];
        if prefix.is_empty() || MAGIC.iter().any(|magic| prefix.starts_with(magic)) {
            return AdmissionDecision::Reject;
        }
        let mut counts = [0_usize; 256];
        for byte in prefix {
            counts[usize::from(*byte)] += 1;
        }
        let entropy = counts
            .iter()
            .filter(|count| **count > 0)
            .map(|count| {
                let p = *count as f64 / prefix.len() as f64;
                -p * p.log2()
            })
            .sum::<f64>();
        if entropy < 6.0 {
            AdmissionDecision::Reject
        } else {
            AdmissionDecision::Accept
        }
    }
}

pub(crate) struct AdmissionCheck {
    filter: Option<Arc<dyn AdmissionFilter>>,
    prefix: Vec<u8>,
    required: usize,
}

impl AdmissionCheck {
    pub(crate) fn new(filter: Option<Arc<dyn AdmissionFilter>>, size: u64) -> Self {
        Self {
            filter,
            prefix: Vec::new(),
            required: size.min(ADMISSION_PREFIX_BYTES as u64) as usize,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<(), crate::StoreError> {
        let Some(filter) = self.filter.as_ref() else {
            return Ok(());
        };
        let needed = self.required - self.prefix.len();
        self.prefix
            .extend_from_slice(&bytes[..bytes.len().min(needed)]);
        if self.prefix.len() == self.required {
            let decision = filter.inspect(&self.prefix);
            self.filter = None;
            self.prefix.clear();
            if decision == AdmissionDecision::Reject {
                return Err(crate::StoreError::AdmissionRejected);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posture_is_bounded_and_does_not_claim_to_prove_encryption() {
        let high_entropy: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
        assert_eq!(
            SealedParcelsOnly.inspect(&high_entropy),
            AdmissionDecision::Accept
        );
        assert_eq!(
            SealedParcelsOnly.inspect(&[0; 4096]),
            AdmissionDecision::Reject
        );
        assert_eq!(SealedParcelsOnly.inspect(&[]), AdmissionDecision::Reject);
        let mut png = high_entropy.clone();
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        assert_eq!(SealedParcelsOnly.inspect(&png), AdmissionDecision::Reject);
        let mut embedded = high_entropy;
        embedded[100..108].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            SealedParcelsOnly.inspect(&embedded),
            AdmissionDecision::Accept
        );
    }
}

//! Transport-neutral Blossom storage, retention and repair primitives.

pub mod auth;
pub mod blossom;
pub mod fetch;
pub mod store;

pub use blossom::{
    AppState, BlossomConfig, BlossomConfigError, FriendGrant, RepairError, RepairReport,
    ServerMetadata, router,
};
pub use fetch::{
    BlobFetcher, FetchConfigError, FetchError, FetchPath, FetchRequest, FetchedBlob, TorHttpFetcher,
};
pub use store::{
    BlobMetadata, ClaimMetadata, ClaimSpec, DeleteOutcome, EvictionRecord, IntegrityReport,
    RepairCandidate, RepairReservation, RetentionTier, Store, StoreConfig, StoreError,
};

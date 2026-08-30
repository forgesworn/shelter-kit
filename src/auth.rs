use crate::store::normalised_class;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use nostr::prelude::Event;
use serde::Deserialize;
use std::{collections::BTreeSet, time::Duration};
use thiserror::Error;
use url::Url;

const BLOSSOM_AUTH_KIND: u16 = 24_242;
const MAX_AUTH_BYTES: usize = 16 * 1024;
const MAX_CONTENT_BYTES: usize = 1024;

#[derive(Debug, Clone)]
pub struct AuthPolicy {
    accepted_servers: BTreeSet<ServerName>,
    allowed_pubkeys: BTreeSet<String>,
    allow_public_writes: bool,
    max_event_lifetime: Duration,
    future_clock_skew: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedUpload {
    pub owner_pubkey: String,
    pub event_id: String,
    pub expires_at: u64,
    /// The advisory `class` this event carried, if it carried exactly one
    /// well-formed tag within bounds.
    ///
    /// No tag, a second tag, a malformed tag or a value outside the bound all
    /// read as `None`, and none of them refuses the event: BUD-11's
    /// duplicate-singleton rule covers the tags that carry authority, and this
    /// one carries none.
    pub class: Option<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthError {
    #[error("missing Blossom authorisation")]
    Missing,
    #[error("invalid Blossom authorisation encoding")]
    Encoding,
    #[error("invalid Nostr authorisation event")]
    InvalidEvent,
    #[error("invalid Nostr authorisation signature")]
    InvalidSignature,
    #[error("the signing public key is not allowed to write to this node")]
    PubkeyNotAllowed,
    #[error("authorisation event must be kind 24242")]
    WrongKind,
    #[error("authorisation event is from the future")]
    FutureEvent,
    #[error("authorisation event has expired")]
    Expired,
    #[error("authorisation lifetime is too long")]
    LifetimeTooLong,
    #[error("authorisation is not scoped to this upload")]
    WrongVerb,
    #[error("authorisation is not scoped to this blob")]
    WrongHash,
    #[error("authorisation is not scoped to this server")]
    WrongServer,
    #[error("authorisation signer does not match the requested public key")]
    WrongPubkey,
    #[error("authorisation event contains ambiguous security tags")]
    AmbiguousTags,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEvent {
    id: String,
    pubkey: String,
    created_at: u64,
    kind: u16,
    tags: Vec<Vec<String>>,
    content: String,
    #[serde(rename = "sig")]
    _sig: String,
}

impl AuthPolicy {
    pub fn new<I, S>(servers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            accepted_servers: servers
                .into_iter()
                .map(Into::into)
                // Configured names tolerate careless case; a `server` tag on
                // the wire does not (BUD-11 requires a lowercase host), so
                // lower-casing happens only on this, the configuration side.
                .filter_map(|server| server_name(&server.to_ascii_lowercase()))
                .collect(),
            allowed_pubkeys: BTreeSet::new(),
            allow_public_writes: false,
            max_event_lifetime: Duration::from_secs(5 * 60),
            future_clock_skew: Duration::from_secs(30),
        }
    }

    pub fn with_allowed_pubkeys<I, S>(mut self, pubkeys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_pubkeys = pubkeys
            .into_iter()
            .map(Into::into)
            .map(|pubkey| pubkey.to_ascii_lowercase())
            .collect();
        self
    }

    pub fn with_public_writes(mut self, allowed: bool) -> Self {
        self.allow_public_writes = allowed;
        self
    }

    pub fn with_max_event_lifetime(mut self, lifetime: Duration) -> Self {
        self.max_event_lifetime = lifetime;
        self
    }

    pub fn verify_upload(
        &self,
        authorization: Option<&str>,
        expected_hash: &str,
        now: u64,
    ) -> Result<VerifiedUpload, AuthError> {
        self.verify_operation(authorization, Some(expected_hash), "upload", now)
    }

    pub fn verify_delete(
        &self,
        authorization: Option<&str>,
        expected_hash: &str,
        now: u64,
    ) -> Result<VerifiedUpload, AuthError> {
        self.verify_operation(authorization, Some(expected_hash), "delete", now)
    }

    pub fn verify_list(
        &self,
        authorization: Option<&str>,
        expected_pubkey: &str,
        now: u64,
    ) -> Result<VerifiedUpload, AuthError> {
        let verified = self.verify_operation(authorization, None, "list", now)?;
        if verified.owner_pubkey != expected_pubkey {
            return Err(AuthError::WrongPubkey);
        }
        Ok(verified)
    }

    fn verify_operation(
        &self,
        authorization: Option<&str>,
        expected_hash: Option<&str>,
        expected_verb: &str,
        now: u64,
    ) -> Result<VerifiedUpload, AuthError> {
        let authorization = authorization.ok_or(AuthError::Missing)?;
        let encoded = authorization
            .strip_prefix("Nostr ")
            .ok_or(AuthError::Encoding)?;
        if encoded.is_empty() || encoded.len() > MAX_AUTH_BYTES * 2 {
            return Err(AuthError::Encoding);
        }
        // BUD-01 specifies standard base64. Decode leniently so a spec-compliant
        // client, a url-safe client, and padded or unpadded input all verify;
        // the alphabets only differ at two characters, so there is no ambiguity.
        let bytes = STANDARD
            .decode(encoded)
            .or_else(|_| STANDARD_NO_PAD.decode(encoded))
            .or_else(|_| URL_SAFE.decode(encoded))
            .or_else(|_| URL_SAFE_NO_PAD.decode(encoded))
            .map_err(|_| AuthError::Encoding)?;
        if bytes.len() > MAX_AUTH_BYTES {
            return Err(AuthError::Encoding);
        }

        let raw: RawEvent = serde_json::from_slice(&bytes).map_err(|_| AuthError::InvalidEvent)?;
        if raw.content.len() > MAX_CONTENT_BYTES || raw.content.trim().is_empty() {
            return Err(AuthError::InvalidEvent);
        }
        let event: Event = serde_json::from_slice(&bytes).map_err(|_| AuthError::InvalidEvent)?;
        event.verify().map_err(|_| AuthError::InvalidSignature)?;

        if !self.allow_public_writes && !self.allowed_pubkeys.contains(&raw.pubkey) {
            return Err(AuthError::PubkeyNotAllowed);
        }

        if raw.kind != BLOSSOM_AUTH_KIND {
            return Err(AuthError::WrongKind);
        }
        if raw.created_at > now.saturating_add(self.future_clock_skew.as_secs()) {
            return Err(AuthError::FutureEvent);
        }

        let verb = unique_singleton_tag(&raw.tags, "t")?;
        if verb != expected_verb {
            return Err(AuthError::WrongVerb);
        }
        if let Some(expected_hash) = expected_hash {
            let hashes = scoped_tags(&raw.tags, "x")?;
            if hashes.is_empty()
                || hashes.iter().any(|hash| !is_canonical_hash(hash))
                || !hashes.contains(&expected_hash)
            {
                return Err(AuthError::WrongHash);
            }
        }
        let servers = scoped_tags(&raw.tags, "server")?;
        let server_names = servers
            .iter()
            .map(|server| server_name(server))
            .collect::<Option<Vec<_>>>();
        let accepted = match &server_names {
            Some(names) if !names.is_empty() => names.iter().any(|name| {
                self.accepted_servers
                    .iter()
                    .any(|accepted| accepted.admits(name))
            }),
            _ => false,
        };
        if servers.is_empty() || self.accepted_servers.is_empty() || !accepted {
            return Err(AuthError::WrongServer);
        }
        let expires_at = unique_singleton_tag(&raw.tags, "expiration")?
            .parse::<u64>()
            .map_err(|_| AuthError::AmbiguousTags)?;
        if expires_at <= now {
            return Err(AuthError::Expired);
        }
        if expires_at < raw.created_at
            || expires_at.saturating_sub(raw.created_at) > self.max_event_lifetime.as_secs()
        {
            return Err(AuthError::LifetimeTooLong);
        }

        Ok(VerifiedUpload {
            owner_pubkey: raw.pubkey,
            event_id: raw.id,
            expires_at,
            class: optional_class_tag(&raw.tags),
        })
    }
}

fn unique_singleton_tag<'a>(tags: &'a [Vec<String>], name: &str) -> Result<&'a str, AuthError> {
    let matches = tags
        .iter()
        .filter(|tag| tag.first().is_some_and(|kind| kind == name))
        .collect::<Vec<_>>();
    if matches.len() != 1 || matches[0].len() != 2 {
        return Err(AuthError::AmbiguousTags);
    }
    Ok(matches[0][1].as_str())
}

fn scoped_tags<'a>(tags: &'a [Vec<String>], name: &str) -> Result<Vec<&'a str>, AuthError> {
    let matches = tags
        .iter()
        .filter(|tag| tag.first().is_some_and(|kind| kind == name))
        .collect::<Vec<_>>();
    if matches.len() > 32 || matches.iter().any(|tag| tag.len() != 2) {
        return Err(AuthError::AmbiguousTags);
    }
    Ok(matches.iter().map(|tag| tag[1].as_str()).collect())
}

/// The single `class` tag on an authorising event, when there is exactly one
/// well-formed tag whose value is within the store's bound.
fn optional_class_tag(tags: &[Vec<String>]) -> Option<String> {
    let mut matches = tags
        .iter()
        .filter(|tag| tag.first().is_some_and(|kind| kind == "class"));
    let only = matches.next().filter(|tag| tag.len() == 2)?;
    matches.next().is_none().then_some(())?;
    normalised_class(Some(only[1].as_str()))
}

fn is_canonical_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// A `server` tag's identity, canonicalised the way BUD-11 §4 and joint core
/// contract item B12 compare it.
///
/// A value is well-formed if it is either a bare lowercase domain name
/// (`node.example`) or an absolute URL with scheme `http` or `https`, a
/// host, an optional port, and nothing else of significance -- a path of
/// `/` or empty is allowed, but a query, a fragment or userinfo is not.
///
/// A bare name canonicalises to its lowercased host alone; no scheme is
/// implied, so it is compared by host only, whatever scheme or port the
/// other side carries. A URL canonicalises to `scheme://host[:port]`, with
/// the host lowercased and the scheme's default port elided (`:443` for
/// `https`, `:80` for `http`). Comparing two URLs requires the same
/// canonical form; comparing a URL against a bare name, in either
/// direction, compares hosts only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ServerName {
    Bare(String),
    Url {
        scheme: String,
        host: String,
        port: Option<u16>,
    },
}

impl ServerName {
    fn host(&self) -> &str {
        match self {
            ServerName::Bare(host) => host,
            ServerName::Url { host, .. } => host,
        }
    }

    /// True when `self`, taken as a configured accepted name, admits `tag`,
    /// a `server` tag's canonical form, under the matching rule above.
    fn admits(&self, tag: &ServerName) -> bool {
        match (self, tag) {
            (ServerName::Url { .. }, ServerName::Url { .. }) => self == tag,
            _ => self.host() == tag.host(),
        }
    }
}

/// Parses a `server` tag value (or a configured accepted name) into its
/// canonical [`ServerName`], or `None` if it is neither a well-formed bare
/// domain name nor a well-formed `http`/`https` URL.
fn server_name(value: &str) -> Option<ServerName> {
    if is_domain_name(value) {
        return Some(ServerName::Bare(value.to_owned()));
    }
    let url = Url::parse(value).ok()?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    Some(ServerName::Url {
        scheme: scheme.to_ascii_lowercase(),
        host,
        // The `url` crate itself elides a scheme's default port during
        // parsing, so `:443` on `https` and `:80` on `http` already come
        // back as `None` here.
        port: url.port(),
    })
}

fn is_domain_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value == value.to_ascii_lowercase()
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use nostr::prelude::{EventBuilder, FinalizeEvent, Keys, Kind, Tag, Timestamp};

    fn header(tags: Vec<Vec<String>>, created_at: u64) -> String {
        header_with_content(tags, created_at, "Upload blob")
    }

    fn header_with_content(tags: Vec<Vec<String>>, created_at: u64, content: &str) -> String {
        let keys = Keys::parse(&format!("{:064x}", 1)).unwrap();
        let tags = tags
            .into_iter()
            .map(|tag| Tag::parse(tag).unwrap())
            .collect::<Vec<_>>();
        let event = EventBuilder::new(Kind::Custom(BLOSSOM_AUTH_KIND), content)
            .tags(tags)
            .custom_created_at(Timestamp::from(created_at))
            .finalize(&keys)
            .unwrap();
        let json = serde_json::to_vec(&event).unwrap();
        format!("Nostr {}", URL_SAFE_NO_PAD.encode(json))
    }

    /// Same event, encoded with standard padded base64 as BUD-01 specifies.
    fn header_standard(tags: Vec<Vec<String>>, created_at: u64) -> String {
        let keys = Keys::parse(&format!("{:064x}", 1)).unwrap();
        let tags = tags
            .into_iter()
            .map(|tag| Tag::parse(tag).unwrap())
            .collect::<Vec<_>>();
        let event = EventBuilder::new(Kind::Custom(BLOSSOM_AUTH_KIND), "Upload blob")
            .tags(tags)
            .custom_created_at(Timestamp::from(created_at))
            .finalize(&keys)
            .unwrap();
        let json = serde_json::to_vec(&event).unwrap();
        format!("Nostr {}", STANDARD.encode(json))
    }

    fn valid_tags(hash: &str, expiration: u64) -> Vec<Vec<String>> {
        vec![
            vec!["t".into(), "upload".into()],
            vec!["x".into(), hash.into()],
            vec!["server".into(), "node.example".into()],
            vec!["expiration".into(), expiration.to_string()],
        ]
    }

    fn public_policy() -> AuthPolicy {
        AuthPolicy::new(["node.example"]).with_public_writes(true)
    }

    #[test]
    fn a_class_tag_is_advisory_and_never_refuses_an_event() {
        let hash = "a".repeat(64);
        let policy = public_policy();
        let verify = |tags: Vec<Vec<String>>| {
            policy
                .verify_upload(Some(&header(tags, 1_000)), &hash, 1_010)
                .unwrap()
                .class
        };

        assert_eq!(verify(valid_tags(&hash, 1_120)), None);

        let mut tagged = valid_tags(&hash, 1_120);
        tagged.push(vec!["class".into(), "vital".into()]);
        assert_eq!(verify(tagged.clone()), Some("vital".to_owned()));

        // A second class tag is ambiguous, so the core reads no class at all --
        // and still accepts the event, because the tag is advisory.
        let mut duplicated = tagged.clone();
        duplicated.push(vec!["class".into(), "working".into()]);
        assert_eq!(verify(duplicated), None);

        // So is a malformed tag or a value outside the bound.
        let mut malformed = valid_tags(&hash, 1_120);
        malformed.push(vec!["class".into(), "vital".into(), "extra".into()]);
        assert_eq!(verify(malformed), None);

        let mut shouty = valid_tags(&hash, 1_120);
        shouty.push(vec!["class".into(), "VITAL".into()]);
        assert_eq!(verify(shouty), None);
    }

    #[test]
    fn accepts_a_valid_exactly_scoped_upload() {
        let hash = "a".repeat(64);
        let auth = header(valid_tags(&hash, 1_120), 1_000);
        let verified = public_policy()
            .verify_upload(Some(&auth), &hash, 1_010)
            .unwrap();
        assert_eq!(verified.expires_at, 1_120);
        assert_eq!(verified.owner_pubkey.len(), 64);
    }

    #[test]
    fn accepts_standard_base64_authorization() {
        // BUD-01 uses standard base64; a spec-compliant client must verify, not
        // only a url-safe one.
        let hash = "a".repeat(64);
        let auth = header_standard(valid_tags(&hash, 1_120), 1_000);
        let verified = public_policy()
            .verify_upload(Some(&auth), &hash, 1_010)
            .unwrap();
        assert_eq!(verified.expires_at, 1_120);
    }

    #[test]
    fn rejects_wrong_hash_server_and_verb() {
        let hash = "a".repeat(64);
        let policy = public_policy();

        let wrong_hash = header(valid_tags(&"b".repeat(64), 1_120), 1_000);
        assert_eq!(
            policy.verify_upload(Some(&wrong_hash), &hash, 1_010),
            Err(AuthError::WrongHash)
        );

        let mut tags = valid_tags(&hash, 1_120);
        tags[2][1] = "attacker.example".into();
        assert_eq!(
            policy.verify_upload(Some(&header(tags, 1_000)), &hash, 1_010),
            Err(AuthError::WrongServer)
        );

        let mut tags = valid_tags(&hash, 1_120);
        tags[0][1] = "delete".into();
        assert_eq!(
            policy.verify_upload(Some(&header(tags, 1_000)), &hash, 1_010),
            Err(AuthError::WrongVerb)
        );
    }

    #[test]
    fn rejects_expired_long_lived_and_ambiguous_singleton_events() {
        let hash = "a".repeat(64);
        let policy = public_policy();

        let expired = header(valid_tags(&hash, 1_009), 1_000);
        assert_eq!(
            policy.verify_upload(Some(&expired), &hash, 1_010),
            Err(AuthError::Expired)
        );

        let long_lived = header(valid_tags(&hash, 1_400), 1_000);
        assert_eq!(
            policy.verify_upload(Some(&long_lived), &hash, 1_010),
            Err(AuthError::LifetimeTooLong)
        );

        let mut tags = valid_tags(&hash, 1_120);
        tags.push(vec!["t".into(), "upload".into()]);
        assert_eq!(
            policy.verify_upload(Some(&header(tags, 1_000)), &hash, 1_010),
            Err(AuthError::AmbiguousTags)
        );
    }

    #[test]
    fn accepts_standard_multi_server_and_multi_hash_scope() {
        let hash = "a".repeat(64);
        let mut tags = valid_tags(&hash, 1_120);
        tags.push(vec!["server".into(), "second.example".into()]);
        tags.push(vec!["x".into(), "b".repeat(64)]);
        assert!(
            public_policy()
                .verify_upload(Some(&header(tags, 1_000)), &hash, 1_010)
                .is_ok()
        );
    }

    #[test]
    fn rejects_tampered_signatures() {
        let hash = "a".repeat(64);
        let auth = header(valid_tags(&hash, 1_120), 1_000);
        let encoded = auth.strip_prefix("Nostr ").unwrap();
        let mut event: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap();
        event["content"] = "Tampered".into();
        let tampered = format!(
            "Nostr {}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&event).unwrap())
        );
        assert_eq!(
            public_policy().verify_upload(Some(&tampered), &hash, 1_010),
            Err(AuthError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_an_empty_human_readable_purpose() {
        let hash = "a".repeat(64);
        let auth = header_with_content(valid_tags(&hash, 1_120), 1_000, "   ");
        assert_eq!(
            public_policy().verify_upload(Some(&auth), &hash, 1_010),
            Err(AuthError::InvalidEvent)
        );
    }

    #[test]
    fn defaults_to_deny_and_accepts_only_configured_writers() {
        let hash = "a".repeat(64);
        let auth = header(valid_tags(&hash, 1_120), 1_000);
        let encoded = auth.strip_prefix("Nostr ").unwrap();
        let event: RawEvent =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap();

        assert_eq!(
            AuthPolicy::new(["node.example"]).verify_upload(Some(&auth), &hash, 1_010),
            Err(AuthError::PubkeyNotAllowed)
        );
        assert!(
            AuthPolicy::new(["node.example"])
                .with_allowed_pubkeys([event.pubkey])
                .verify_upload(Some(&auth), &hash, 1_010)
                .is_ok()
        );
    }

    #[test]
    fn delete_requires_the_delete_verb() {
        let hash = "a".repeat(64);
        let upload = header(valid_tags(&hash, 1_120), 1_000);
        let mut delete_tags = valid_tags(&hash, 1_120);
        delete_tags[0][1] = "delete".into();
        let delete = header(delete_tags, 1_000);

        assert_eq!(
            public_policy().verify_delete(Some(&upload), &hash, 1_010),
            Err(AuthError::WrongVerb)
        );
        assert!(
            public_policy()
                .verify_delete(Some(&delete), &hash, 1_010)
                .is_ok()
        );
    }

    #[test]
    fn a_url_shaped_server_tag_is_accepted() {
        let hash = "a".repeat(64);
        let policy = AuthPolicy::new(["https://node.example"]).with_public_writes(true);
        let mut tags = valid_tags(&hash, 1_120);
        tags[2][1] = "https://node.example".into();
        let auth = header(tags, 1_000);
        assert!(policy.verify_upload(Some(&auth), &hash, 1_010).is_ok());
    }

    #[test]
    fn a_bare_host_matches_any_scheme_and_port() {
        let hash = "a".repeat(64);
        let policy = AuthPolicy::new(["node.example"]).with_public_writes(true);
        for tag_value in [
            "http://node.example:8080",
            "https://node.example",
            "node.example",
        ] {
            let mut tags = valid_tags(&hash, 1_120);
            tags[2][1] = tag_value.into();
            let auth = header(tags, 1_000);
            assert!(
                policy.verify_upload(Some(&auth), &hash, 1_010).is_ok(),
                "expected {tag_value:?} to match the bare accepted name"
            );
        }
    }

    #[test]
    fn a_url_with_a_path_query_or_userinfo_is_malformed() {
        let hash = "a".repeat(64);
        let policy = AuthPolicy::new(["node.example"]).with_public_writes(true);
        for tag_value in [
            "https://node.example/blob",
            "https://node.example/?x=1",
            "https://node.example/#frag",
            "https://user@node.example/",
            "https://user:pass@node.example/",
        ] {
            let mut tags = valid_tags(&hash, 1_120);
            tags[2][1] = tag_value.into();
            let auth = header(tags, 1_000);
            assert_eq!(
                policy.verify_upload(Some(&auth), &hash, 1_010),
                Err(AuthError::WrongServer),
                "expected {tag_value:?} to be rejected as malformed"
            );
        }
    }

    #[test]
    fn a_url_for_another_host_is_wrong_server() {
        let hash = "a".repeat(64);
        let policy = AuthPolicy::new(["https://node.example"]).with_public_writes(true);
        let mut tags = valid_tags(&hash, 1_120);
        tags[2][1] = "https://attacker.example".into();
        let auth = header(tags, 1_000);
        assert_eq!(
            policy.verify_upload(Some(&auth), &hash, 1_010),
            Err(AuthError::WrongServer)
        );
    }

    #[test]
    fn default_ports_are_elided_in_the_canonical_form() {
        let hash = "a".repeat(64);

        let https_policy = AuthPolicy::new(["https://node.example"]).with_public_writes(true);
        let mut https_tags = valid_tags(&hash, 1_120);
        https_tags[2][1] = "https://node.example:443".into();
        let https_auth = header(https_tags, 1_000);
        assert!(
            https_policy
                .verify_upload(Some(&https_auth), &hash, 1_010)
                .is_ok()
        );

        let http_policy = AuthPolicy::new(["http://node.example"]).with_public_writes(true);
        let mut http_tags = valid_tags(&hash, 1_120);
        http_tags[2][1] = "http://node.example:80".into();
        let http_auth = header(http_tags, 1_000);
        assert!(
            http_policy
                .verify_upload(Some(&http_auth), &hash, 1_010)
                .is_ok()
        );
    }

    #[test]
    fn a_stash_rs_minted_server_tag_is_accepted() {
        // `stash-rs`'s BUD-11 signer mints `TagStandard::Server(url)` from a
        // `url::Url`; its serialisation always carries the trailing slash of a
        // path-less URL, so this is the literal tag value it produces on the
        // wire against a bare configured name.
        let hash = "a".repeat(64);
        let policy = AuthPolicy::new(["node.example"]).with_public_writes(true);
        let mut tags = valid_tags(&hash, 1_120);
        tags[2][1] = "https://node.example/".into();
        let auth = header(tags, 1_000);
        assert!(policy.verify_upload(Some(&auth), &hash, 1_010).is_ok());
    }
}

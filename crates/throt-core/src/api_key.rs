use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Metadata for a registered API key. Secret is never stored or returned after creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiKeyMetadata {
    pub id: Uuid,
    pub application_id: Uuid,
    pub key_prefix: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Newly generated API key containing the plaintext secret (returned only once).
#[derive(Debug, Clone, Serialize)]
pub struct GeneratedApiKey {
    pub id: Uuid,
    pub application_id: Uuid,
    pub raw_key: String,
    pub key_prefix: String,
    #[serde(skip_serializing)]
    pub key_hash: String,
    pub created_at: DateTime<Utc>,
}

impl GeneratedApiKey {
    /// Generates a cryptographically secure 256-bit API key.
    pub fn generate(application_id: Uuid) -> Self {
        let mut random_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut random_bytes);
        let token = hex::encode(random_bytes);
        let raw_key = format!("throt_live_{token}");
        let key_prefix = raw_key[..15].to_string();
        let key_hash = Self::hash_key(&raw_key);

        Self {
            id: Uuid::new_v4(),
            application_id,
            raw_key,
            key_prefix,
            key_hash,
            created_at: Utc::now(),
        }
    }

    /// Computes the SHA-256 hash of a raw API key.
    pub fn hash_key(raw_key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(raw_key.trim().as_bytes());
        hex::encode(hasher.finalize())
    }
}

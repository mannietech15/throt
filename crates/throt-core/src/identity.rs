use serde::{Deserialize, Serialize};

/// Target identity being rate-limited.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Identity {
    raw: String,
}

impl Identity {
    /// Creates a validated, normalized identity.
    pub fn new(val: impl Into<String>) -> Result<Self, &'static str> {
        let s = val.into().trim().to_string();
        if s.is_empty() {
            return Err("Identity cannot be empty");
        }
        if s.len() > 256 {
            return Err("Identity length exceeds 256 characters");
        }
        Ok(Self { raw: s })
    }

    /// Returns the raw normalized identity string.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.raw)
    }
}

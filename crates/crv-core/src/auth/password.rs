use crv_shared::error::{CrvError, Result};

/// Hash a plaintext password using bcrypt.
pub fn hash_password(password: &str) -> Result<String> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST)
        .map_err(|e| CrvError::Internal(format!("password hashing failed: {e}")))
}

/// Verify a plaintext password against a bcrypt hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
    bcrypt::verify(password, hash)
        .map_err(|e| CrvError::Internal(format!("password verification failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let password = "super-secret-123";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("wrong-password", &hash).unwrap());
    }

    #[test]
    fn test_different_passwords_produce_different_hashes() {
        let h1 = hash_password("a").unwrap();
        let h2 = hash_password("b").unwrap();
        assert_ne!(h1, h2);
    }
}

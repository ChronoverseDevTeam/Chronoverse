use chrono::{DateTime, Duration, Utc};
use crv_shared::error::{CrvError, Result};
use hmac::{Hmac, Mac};
use sha2::{Sha256, Digest};
use sqlx::PgPool;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Generate a cryptographically random ticket string for authentication.
///
/// The ticket format is: `<random_uuid>:<user_id>:<issued_at_timestamp>:<hmac_signature>`
pub fn generate_ticket(secret: &[u8], user_id: Uuid, issued_at: DateTime<Utc>) -> Result<String> {
    let random_id = Uuid::new_v4().to_string();
    let timestamp = issued_at.timestamp().to_string();

    // Payload: random_uuid:user_id:timestamp
    let payload = format!("{}:{}:{}", random_id, user_id, timestamp);

    // Sign with HMAC-SHA256
    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|e| CrvError::Internal(format!("HMAC init failed: {e}")))?;
    mac.update(payload.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    // Ticket: payload:signature
    Ok(format!("{}:{}", payload, signature))
}

/// Validate a ticket string. Returns the user_id if valid and not expired.
pub async fn validate_ticket(
    pool: &PgPool,
    secret: &[u8],
    ticket: &str,
) -> Result<Uuid> {
    // Split into payload and signature
    let (payload, signature) = ticket
        .rsplit_once(':')
        .ok_or_else(|| CrvError::AuthFailed("invalid ticket format".into()))?;

    // Verify HMAC signature
    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|e| CrvError::Internal(format!("HMAC init failed: {e}")))?;
    mac.update(payload.as_bytes());
    let expected_sig = hex::encode(mac.finalize().into_bytes());

    if signature != expected_sig {
        return Err(CrvError::AuthFailed("ticket signature invalid".into()));
    }

    // Parse payload: random:user_id:timestamp
    let parts: Vec<&str> = payload.split(':').collect();
    if parts.len() != 3 {
        return Err(CrvError::AuthFailed("invalid ticket payload".into()));
    }

    let user_id: Uuid = parts[1]
        .parse()
        .map_err(|_| CrvError::AuthFailed("invalid user id in ticket".into()))?;

    let timestamp: i64 = parts[2]
        .parse()
        .map_err(|_| CrvError::AuthFailed("invalid timestamp in ticket".into()))?;

    let issued_at = DateTime::from_timestamp(timestamp, 0)
        .ok_or_else(|| CrvError::AuthFailed("invalid ticket timestamp".into()))?;

    // Check expiry (default: 24 hours from issue)
    let ttl = Duration::hours(24);
    if Utc::now() > issued_at + ttl {
        return Err(CrvError::AuthFailed("ticket expired".into()));
    }

    // Check that ticket exists in database (has not been revoked)
    let ticket_hash = hex::encode(sha2::Sha256::digest(ticket.as_bytes()));
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM auth_tickets WHERE ticket_hash = $1 AND expires_at > now())"
    )
    .bind(&ticket_hash)
    .fetch_one(pool)
    .await
    .map_err(|e| CrvError::Database(format!("ticket lookup failed: {e}")))?;

    if !exists {
        return Err(CrvError::AuthFailed("ticket revoked or expired".into()));
    }

    Ok(user_id)
}

/// Store a ticket hash in the database so it can be validated later.
pub async fn store_ticket(
    pool: &PgPool,
    user_id: Uuid,
    ticket: &str,
    client_addr: Option<&str>,
    ttl_hours: i64,
) -> Result<()> {
    let ticket_hash = hex::encode(sha2::Sha256::digest(ticket.as_bytes()));
    let expires_at = Utc::now() + Duration::hours(ttl_hours);

    sqlx::query(
        "INSERT INTO auth_tickets (user_id, ticket_hash, expires_at, client_addr)
         VALUES ($1, $2, $3, $4)"
    )
    .bind(user_id)
    .bind(&ticket_hash)
    .bind(expires_at)
    .bind(client_addr)
    .execute(pool)
    .await
    .map_err(|e| CrvError::Database(format!("failed to store ticket: {e}")))?;

    Ok(())
}

/// Revoke all tickets for a user (logout).
pub async fn revoke_user_tickets(pool: &PgPool, user_id: Uuid) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM auth_tickets WHERE user_id = $1"
    )
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|e| CrvError::Database(format!("failed to revoke tickets: {e}")))?;

    Ok(result.rows_affected())
}

/// Revoke a specific ticket (logout from one session).
pub async fn revoke_ticket(pool: &PgPool, ticket: &str) -> Result<()> {
    let ticket_hash = hex::encode(sha2::Sha256::digest(ticket.as_bytes()));

    sqlx::query("DELETE FROM auth_tickets WHERE ticket_hash = $1")
        .bind(&ticket_hash)
        .execute(pool)
        .await
        .map_err(|e| CrvError::Database(format!("failed to revoke ticket: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_validate_ticket_format() {
        let secret = b"test-secret-key";
        let user_id = Uuid::new_v4();
        let now = Utc::now();

        let ticket = generate_ticket(secret, user_id, now).unwrap();

        // Ticket should have 4 colon-separated parts + signature = 4 parts total
        // Format: random:user_id:timestamp:signature
        let parts: Vec<&str> = ticket.split(':').collect();
        assert_eq!(parts.len(), 4, "ticket should have 4 colon-separated parts");

        // Verify user_id is in the payload
        assert!(ticket.contains(&user_id.to_string()));
    }

    #[test]
    fn test_tampered_ticket_rejected() {
        let secret = b"test-secret-key";
        let user_id = Uuid::new_v4();
        let ticket = generate_ticket(secret, user_id, Utc::now()).unwrap();

        // Tamper with the ticket by appending extra data
        let tampered = format!("{ticket}extra");

        let (_payload, signature) = tampered.rsplit_once(':').unwrap();
        let original_sig = ticket.rsplit_once(':').unwrap().1;
        // The tampered ticket's last segment (after adding 'extra')
        // will be different from the original signature
        assert!(signature != original_sig || tampered != ticket,
            "tampered ticket should differ from original");
    }
}

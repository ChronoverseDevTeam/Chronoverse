use crv_core::cas::{BlobId, CasError, CasStore};
use iroh::EndpointAddr;
use iroh_blobs::{BlobFormat, ticket::BlobTicket};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct BlobTicketOffer {
	pub hash: String,
	pub ticket: BlobTicket,
}

#[derive(Debug, Error)]
pub enum FileServiceError {
	#[error("blob hash must not be empty")]
	EmptyHash,
	#[error("invalid blob hash: {0}")]
	InvalidHash(String),
	#[error("blob not found: {0}")]
	NotFound(String),
	#[error("cas error: {0}")]
	Cas(#[from] CasError),
}

pub async fn create_blob_ticket(
	store: &CasStore,
	provider_addr: &EndpointAddr,
	hash: &str,
) -> Result<BlobTicketOffer, FileServiceError> {
	let hash_text = hash.trim();
	if hash_text.is_empty() {
		return Err(FileServiceError::EmptyHash);
	}

	let parsed_hash = blake3::Hash::from_hex(hash_text)
		.map_err(|err| FileServiceError::InvalidHash(err.to_string()))?;
	let blob_id = BlobId::from_bytes(*parsed_hash.as_bytes());

	if !store.exists(blob_id).await? {
		return Err(FileServiceError::NotFound(hash_text.to_owned()));
	}

	let ticket = BlobTicket::new(provider_addr.clone(), blob_id, BlobFormat::Raw);

	Ok(BlobTicketOffer {
		hash: blob_id.to_string(),
		ticket,
	})
}

#[cfg(test)]
mod tests {
	use super::{FileServiceError, create_blob_ticket};
	use crv_core::cas::CasStore;
	use iroh::{EndpointAddr, SecretKey};

	fn test_provider_addr(seed: u8) -> EndpointAddr {
		EndpointAddr::new(SecretKey::from_bytes(&[seed; 32]).public())
	}

	#[tokio::test]
	async fn create_blob_ticket_returns_ticket_for_existing_hash() {
		let store = CasStore::memory();
		let pin = store.put_bytes("hello world").await.expect("blob should store");
		let hash = pin.hash().to_string();
		let provider_addr = test_provider_addr(1);

		let blob = create_blob_ticket(&store, &provider_addr, &hash)
			.await
			.expect("blob ticket should build");

		assert_eq!(blob.hash, hash);
		assert_eq!(blob.ticket.hash().to_string(), hash);
		assert_eq!(blob.ticket.addr(), &provider_addr);
	}

	#[tokio::test]
	async fn create_blob_ticket_rejects_unknown_hash() {
		let store = CasStore::memory();
		let missing_hash = blake3::hash(b"missing").to_string();
		let provider_addr = test_provider_addr(2);

		let error = create_blob_ticket(&store, &provider_addr, &missing_hash)
			.await
			.expect_err("missing blob should fail");

		assert!(matches!(error, FileServiceError::NotFound(_)));
	}

	#[tokio::test]
	async fn create_blob_ticket_rejects_invalid_hash() {
		let store = CasStore::memory();
		let provider_addr = test_provider_addr(3);

		let error = create_blob_ticket(&store, &provider_addr, "not-a-hash")
			.await
			.expect_err("invalid hash should fail");

		assert!(matches!(error, FileServiceError::InvalidHash(_)));
	}
}
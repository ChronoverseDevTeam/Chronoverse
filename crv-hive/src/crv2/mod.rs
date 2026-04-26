pub mod config;
pub mod postgres;
pub mod service;
pub mod iroh;

use std::collections::HashSet;
use std::sync::Arc;

use crv_core::cas::CasStore;
use ::iroh::EndpointAddr;
use postgres::PostgreExecutor;
use service::{
    BlobTicketOffer,
    PreSubmitFile,
    SubmitServiceError,
    SubmitRegistry,
    file,
    user::{self, CreateUserRequest, UpdateUserPasswordRequest, UserProfile},
};

pub struct ChronoverseApp {
    postgres: Arc<PostgreExecutor>,
	cas_store: CasStore,
	provider_addr: EndpointAddr,
	submit_registry: Arc<SubmitRegistry>,
}

impl ChronoverseApp {
        pub fn new(postgres: Arc<PostgreExecutor>, cas_store: CasStore, provider_addr: EndpointAddr) -> Self {
		ChronoverseApp { postgres, cas_store, provider_addr, submit_registry: Arc::new(SubmitRegistry::new()) }
    }

    pub fn postgres(&self) -> &PostgreExecutor {
        self.postgres.as_ref()
    }

    pub fn postgres_arc(&self) -> Arc<PostgreExecutor> {
        Arc::clone(&self.postgres)
    }

	pub fn cas_store(&self) -> &CasStore {
		&self.cas_store
	}

    pub fn provider_addr(&self) -> &EndpointAddr {
        &self.provider_addr
    }

    pub fn submit_registry(&self) -> &Arc<SubmitRegistry> {
        &self.submit_registry
    }

    pub async fn register_user(&self, username: &str, password: &str) -> Result<UserProfile, String> {
        let user = user::create_user(
            self.postgres(),
            &CreateUserRequest {
                username: username.to_owned(),
                password: password.to_owned(),
            },
        )
        .await
        .map_err(|err| err.to_string())?;

        Ok(user)
    }

    pub async fn update_user_password(
        &self,
        username: &str,
        new_password: &str,
    ) -> Result<(), String> {
        user::update_user_password(
            self.postgres(),
            &UpdateUserPasswordRequest {
                username: username.to_owned(),
                new_password: new_password.to_owned(),
            },
        )
        .await
        .map_err(|err| err.to_string())
    }

    pub async fn get_user(&self, username: &str) -> Result<Option<UserProfile>, String> {
        user::get_user(self.postgres(), username)
            .await
            .map_err(|err| err.to_string())
    }

    pub async fn create_blob_ticket(&self, hash: &str) -> Result<BlobTicketOffer, String> {
        file::create_blob_ticket(self.cas_store(), self.provider_addr(), hash)
			.await
			.map_err(|err| err.to_string())
	}

	pub async fn pre_submit(
		&self,
		description: String,
        files: Vec<PreSubmitFile>,
	) -> Result<service::PreSubmitResult, String> {
		let result = service::submit::pre_submit(
			self.postgres(),
			"test", // TODO: replace with authenticated user
			&description,
			&files,
		)
		.await
		.map_err(|err| err.to_string())?;

        // Track only missing chunks; already-present CAS content does not need upload keepalive.
        let hashes = collect_missing_submit_hashes(self.cas_store(), &files)
            .await
            .map_err(|err| err.to_string())?;
		self.submit_registry.register(result.submit_id, hashes);

		Ok(result)
	}

	pub async fn submit(&self, submit_id: i64) -> Result<service::SubmitResult, String> {
        match service::submit::submit(self.postgres(), self.cas_store(), submit_id).await {
            Ok(result) => {
                self.submit_registry.unregister(submit_id);
                Ok(result)
            }
            Err(err) => {
                if matches!(err, SubmitServiceError::Expired(_)) {
                    self.submit_registry.unregister(submit_id);
                }
                Err(err.to_string())
            }
        }
	}

	pub async fn cancel_submit(&self, submit_id: i64) -> Result<(), String> {
        match service::submit::cancel_submit(self.postgres(), submit_id).await {
            Ok(()) => {
                self.submit_registry.unregister(submit_id);
                Ok(())
            }
            Err(err) => {
                if matches!(err, SubmitServiceError::Expired(_)) {
                    self.submit_registry.unregister(submit_id);
                }
                Err(err.to_string())
            }
        }
	}

}

async fn collect_missing_submit_hashes(
    cas_store: &CasStore,
    files: &[PreSubmitFile],
) -> Result<Vec<iroh_blobs::Hash>, crv_core::cas::CasError> {
    let mut seen = HashSet::new();
    let mut missing = Vec::new();

    for file in files {
        for hash in &file.chunk_hashes {
            let parsed = match blake3::Hash::from_hex(hash) {
                Ok(parsed) => parsed,
                Err(_) => continue,
            };
            let blob_id = crv_core::cas::BlobId::from_bytes(*parsed.as_bytes());
            if !seen.insert(blob_id) {
                continue;
            }
            if !cas_store.exists(blob_id).await? {
                missing.push(blob_id);
            }
        }
    }

    Ok(missing)
}
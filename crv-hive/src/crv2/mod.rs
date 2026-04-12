pub mod config;
pub mod postgres;
pub mod service;
pub mod iroh;

use std::sync::Arc;

use crv_core::cas::CasStore;
use ::iroh::EndpointAddr;
use postgres::PostgreExecutor;
use service::{
    BlobTicketOffer,
    SubmitRegistry,
    file,
    user::{self, CreateUserRequest, UpdateUserPasswordRequest, UserProfile},
};

pub struct RegisterUserReq {
    pub username: String,
    pub password: String,
}

pub struct RegisterUserRsp {
    pub username: String
}

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

    pub async fn register_user(
        self: &ChronoverseApp,
        req: &RegisterUserReq
    ) -> Result<RegisterUserRsp, String> {
        let user = user::create_user(
            self.postgres(),
            &CreateUserRequest {
                username: req.username.clone(),
                password: req.password.clone(),
            },
        )
        .await
        .map_err(|err| err.to_string())?;

        Ok(RegisterUserRsp { username: user.username })
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
		files: Vec<crate::crv2::iroh::controller::pre_submit_controller::PreSubmitFile>,
	) -> Result<service::PreSubmitResult, String> {
		let result = service::submit::pre_submit(
			self.postgres(),
			"test", // TODO: replace with authenticated user
			&description,
			&files,
		)
		.await
		.map_err(|err| err.to_string())?;

		// Register chunk hashes for per-submit expiry tracking.
		let hashes = files.iter()
			.flat_map(|f| f.chunk_hashes.iter())
			.filter_map(|h| {
				blake3::Hash::from_hex(h).ok()
					.map(|bh| crv_core::cas::BlobId::from_bytes(*bh.as_bytes()))
			});
		self.submit_registry.register(result.submit_id, hashes);

		Ok(result)
	}

	pub async fn submit(&self, submit_id: i64) -> Result<service::SubmitResult, String> {
		let result = service::submit::submit(self.postgres(), self.cas_store(), submit_id)
			.await
			.map_err(|err| err.to_string())?;
		self.submit_registry.unregister(submit_id);
		Ok(result)
	}

	pub async fn cancel_submit(&self, submit_id: i64) -> Result<(), String> {
		service::submit::cancel_submit(self.postgres(), submit_id)
			.await
			.map_err(|err| err.to_string())?;
		self.submit_registry.unregister(submit_id);
		Ok(())
	}

}
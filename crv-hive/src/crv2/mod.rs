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
}

impl ChronoverseApp {
        pub fn new(postgres: Arc<PostgreExecutor>, cas_store: CasStore, provider_addr: EndpointAddr) -> Self {
		ChronoverseApp { postgres, cas_store, provider_addr }
    }

    pub fn postgres(&self) -> &PostgreExecutor {
        self.postgres.as_ref()
    }

	pub fn cas_store(&self) -> &CasStore {
		&self.cas_store
	}

    pub fn provider_addr(&self) -> &EndpointAddr {
        &self.provider_addr
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

}
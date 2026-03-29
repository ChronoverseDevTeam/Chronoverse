use argon2::{
	Argon2,
	PasswordHasher,
	password_hash::{SaltString, rand_core::OsRng},
};
use chrono::Utc;
use thiserror::Error;

use crate::crv2::postgres::{
	PostgreExecutor,
	PostgreExecutorError,
	dao::{DaoError, user::{self, NewUser}},
	entity::user::Model as UserModel,
};

#[derive(Debug, Clone)]
pub struct CreateUserRequest {
	pub username: String,
	pub password: String,
}

#[derive(Debug, Clone)]
pub struct UpdateUserPasswordRequest {
	pub username: String,
	pub new_password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserProfile {
	pub username: String,
	pub created_at: i64,
}

#[derive(Debug, Error)]
pub enum UserServiceError {
	#[error("username must not be empty")]
	EmptyUsername,
	#[error("password must not be empty")]
	EmptyPassword,
	#[error("user already exists: {0}")]
	UserAlreadyExists(String),
	#[error("user not found: {0}")]
	UserNotFound(String),
	#[error("password hashing failed: {0}")]
	PasswordHash(String),
	#[error("user service dao error: {0}")]
	Dao(#[from] DaoError),
	#[error("user service executor error: {0}")]
	Executor(#[from] PostgreExecutorError),
}

impl From<UserModel> for UserProfile {
	fn from(model: UserModel) -> Self {
		Self {
			username: model.username,
			created_at: model.created_at,
		}
	}
}

pub async fn create_user(
	executor: &PostgreExecutor,
	request: &CreateUserRequest,
) -> Result<UserProfile, UserServiceError> {
	let username = request.username.trim().to_owned();
	if username.is_empty() {
		return Err(UserServiceError::EmptyUsername);
	}

	if request.password.is_empty() {
		return Err(UserServiceError::EmptyPassword);
	}

	let password_hash = hash_password(&request.password)?;
	let created_at = Utc::now().timestamp_millis();

	executor
		.transaction(|txn| {
			Box::pin(async move {
				if user::find_by_username(txn, &username).await?.is_some() {
					return Err(UserServiceError::UserAlreadyExists(username.clone()));
				}

				user::insert(
					txn,
					NewUser {
						username: username.clone(),
						password_hash,
						created_at,
					},
				)
				.await?;

				Ok(UserProfile {
					username,
					created_at,
				})
			})
		})
		.await
}

pub async fn update_user_password(
	executor: &PostgreExecutor,
	request: &UpdateUserPasswordRequest,
) -> Result<(), UserServiceError> {
	let username = request.username.trim().to_owned();
	if username.is_empty() {
		return Err(UserServiceError::EmptyUsername);
	}

	if request.new_password.is_empty() {
		return Err(UserServiceError::EmptyPassword);
	}

	let new_hash = hash_password(&request.new_password)?;

	executor
		.transaction(|txn| {
			Box::pin(async move {
				if user::find_by_username(txn, &username).await?.is_none() {
					return Err(UserServiceError::UserNotFound(username.clone()));
				}

				user::update_password(txn, &username, &new_hash).await?;
				Ok(())
			})
		})
		.await
}

pub async fn get_user(
	executor: &PostgreExecutor,
	username: &str,
) -> Result<Option<UserProfile>, UserServiceError> {
	let username = username.trim().to_owned();
	if username.is_empty() {
		return Err(UserServiceError::EmptyUsername);
	}

	executor
		.transaction(|txn| {
			Box::pin(async move {
				Ok(user::find_by_username(txn, &username)
					.await?
					.map(UserProfile::from))
			})
		})
		.await
}

fn hash_password(password: &str) -> Result<String, UserServiceError> {
	let salt = SaltString::generate(&mut OsRng);
	Argon2::default()
		.hash_password(password.as_bytes(), &salt)
		.map(|hash| hash.to_string())
		.map_err(|err| UserServiceError::PasswordHash(err.to_string()))
}

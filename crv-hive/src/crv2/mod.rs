use anyhow::Error;

pub mod postgres;
pub mod service;
pub mod iroh;

pub struct RegisterUserReq {
    pub username: String,
    pub password: String,
}

pub struct RegisterUserRsp {
    pub username: String
}

pub struct ChronoverseApp {

}

impl ChronoverseApp {
    pub fn new() -> Self {
        ChronoverseApp {}
    }

    pub fn register_user(
        self: &ChronoverseApp,
        req: &RegisterUserReq
    ) -> Result<RegisterUserRsp, String> {
        // this is a test api
        Ok(RegisterUserRsp {
            username: req.username.clone()
        })
    }

}
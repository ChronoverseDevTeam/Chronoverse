use anyhow::Result;
use clap::Parser;
use tonic::transport::Channel;

#[derive(Parser)]
pub struct AddCli;

impl AddCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}

#[derive(Parser)]
pub struct SubmitCli;

impl SubmitCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}

#[derive(Parser)]
pub struct SyncCli;

impl SyncCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}

#[derive(Parser)]
pub struct RevertCli;

impl RevertCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}

#[derive(Parser)]
pub struct LockCli;

impl LockCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        todo!()
    }
}

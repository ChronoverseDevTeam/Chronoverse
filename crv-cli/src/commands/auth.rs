use anyhow::Result;
use clap::Parser;
use console::style;
use crv_edge::pb::{
    GetAuthStatusReq, LoginReq, LogoutReq, system_service_client::SystemServiceClient,
};
use dialoguer::{Input, Password, theme::ColorfulTheme};
use tonic::transport::Channel;

#[derive(Parser)]
pub struct LoginCli {
    #[arg(short, long)]
    pub username: Option<String>,
}

impl LoginCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let username = match &self.username {
            Some(v) if !v.trim().is_empty() => v.clone(),
            _ => Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt("Username")
                .interact_text()?,
        };

        let password = Password::with_theme(&ColorfulTheme::default())
            .with_prompt("Password")
            .allow_empty_password(false)
            .interact()?;

        let mut client = SystemServiceClient::new(channel.clone());
        let rsp = client
            .login(LoginReq { username, password })
            .await?
            .into_inner();

        println!(
            "{}",
            style(format!("Login success, token expires_at={}", rsp.expires_at)).green()
        );

        Ok(())
    }
}

#[derive(Parser)]
pub struct LogoutCli;

impl LogoutCli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = SystemServiceClient::new(channel.clone());
        let _ = client.logout(LogoutReq {}).await?.into_inner();
        println!("{}", style("Logout success").green());

        Ok(())
    }
}

#[derive(Parser)]
pub struct WhoAmICli;

impl WhoAmICli {
    pub async fn handle(&self, channel: &Channel) -> Result<()> {
        let mut client = SystemServiceClient::new(channel.clone());
        let rsp = client
            .get_auth_status(GetAuthStatusReq {})
            .await?
            .into_inner();

        println!("Current user: {}", style(rsp.current_user).cyan());
        println!(
            "Logged in: {}",
            if rsp.logged_in {
                style("yes").green().to_string()
            } else {
                style("no").yellow().to_string()
            }
        );

        Ok(())
    }
}

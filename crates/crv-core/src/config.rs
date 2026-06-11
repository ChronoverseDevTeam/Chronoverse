use crv_shared::error::{CrvError, Result};

/// Server configuration, populated from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub ticket_secret: String,
    pub depot_root: String,
    pub ticket_ttl_hours: i64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            host: std::env::var("CRV_HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            port: std::env::var("CRV_PORT")
                .unwrap_or_else(|_| "3000".into())
                .parse()
                .map_err(|e| CrvError::InvalidInput(format!("invalid CRV_PORT: {e}")))?,
            database_url: std::env::var("DATABASE_URL")
                .map_err(|_| CrvError::InvalidInput("DATABASE_URL not set".into()))?,
            ticket_secret: std::env::var("CRV_TICKET_SECRET")
                .unwrap_or_else(|_| "change-me-in-production".into()),
            depot_root: std::env::var("CRV_DEPOT_ROOT")
                .unwrap_or_else(|_| "./data/depot".into()),
            ticket_ttl_hours: std::env::var("CRV_TICKET_TTL_HOURS")
                .unwrap_or_else(|_| "24".into())
                .parse()
                .unwrap_or(24),
        })
    }

    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

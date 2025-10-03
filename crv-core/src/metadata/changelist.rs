use chrono::{DateTime, Utc};
use ::serde::{Deserialize, Serialize};

use crate::metadata::{file_revision::MetaFileRevision};

mod chrono_dt_option_as_bson_dt {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};
    use serde::Serialize as SerdeSerialize;

    pub fn serialize<S>(value: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Map Option<chrono::DateTime> -> Option<bson::DateTime> and delegate to Serde
        let mapped: Option<bson::DateTime> = value.map(bson::DateTime::from_chrono);
        SerdeSerialize::serialize(&mapped, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize as Option<bson::DateTime> then map to Option<chrono::DateTime>
        let opt_bson: Option<bson::DateTime> = Option::<bson::DateTime>::deserialize(deserializer)?;
        Ok(opt_bson.map(|bdt| bdt.to_chrono()))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Changelist {
    #[serde(rename = "_id")]
    pub id: u64,
    pub description: String,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(
        with = "chrono_dt_option_as_bson_dt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub submitted_at: Option<DateTime<Utc>>,
    pub owner: String,
    pub files: Vec<MetaFileRevision>
}
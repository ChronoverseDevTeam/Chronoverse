use sea_orm::entity::prelude::*;

/// Status of a submit process.
///
/// - `pending`   – submit has been created; files are locked.
/// - `committed` – all chunks uploaded and changelist finalised.
/// - `cancelled` – user explicitly cancelled the submit.
/// - `expired`   – server reclaimed the submit after timeout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubmitStatus {
    Pending,
    Committed,
    Cancelled,
    Expired,
}

impl SubmitStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Committed => "committed",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "committed" => Some(Self::Committed),
            "cancelled" => Some(Self::Cancelled),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

/// A submit process tracks an in-progress or completed file submission.
///
/// While `status = 'pending'`, the files listed in `submit_files` are
/// exclusively locked — no other submit may include the same depot path.
/// This provides pessimistic locking at the file level.
///
/// On successful commit, the submit transitions to `committed` and a
/// corresponding changelist + file_revisions are created.  The lock is
/// implicitly released when the status leaves `pending`.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "submits")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,

    /// Username of the submitter.
    pub author: String,

    /// Free-form description (may be updated before commit).
    pub description: String,

    /// Current lifecycle status: pending | committed | cancelled | expired.
    pub status: String,

    /// The changelist created on successful commit.
    /// `NULL` while status is not `committed`.
    pub changelist_id: Option<i64>,

    /// Unix timestamp in milliseconds — when this submit was created.
    pub created_at: i64,

    /// Unix timestamp in milliseconds — deadline after which the server may
    /// expire the submit and release all file locks.
    pub expires_at: i64,
}

impl Model {
    pub fn parsed_status(&self) -> Option<SubmitStatus> {
        SubmitStatus::from_str(&self.status)
    }

    pub fn is_pending(&self) -> bool {
        self.status == "pending"
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::submit_file::Entity")]
    SubmitFile,

    #[sea_orm(
        belongs_to = "super::changelist::Entity",
        from = "Column::ChangelistId",
        to = "super::changelist::Column::Id"
    )]
    Changelist,
}

impl Related<super::submit_file::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SubmitFile.def()
    }
}

impl Related<super::changelist::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Changelist.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

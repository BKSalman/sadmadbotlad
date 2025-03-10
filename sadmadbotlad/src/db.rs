use serde::{Deserialize, Serialize};
use surrealdb::{
    engine::local::{Db, RocksDb},
    sql::Datetime,
    RecordId, RecordIdKey, Surreal,
};

use crate::{AlertEventType, APP};

#[derive(thiserror::Error, Debug)]
pub enum DatabaseError {
    #[error(transparent)]
    SurrealDBError(#[from] surrealdb::Error),

    #[error("value not of type Object")]
    NotObject,

    #[error("value not of type Array")]
    NotArray,

    #[error("value not of type i64")]
    NotI64,

    #[error("value not of type bool")]
    NotBool,

    #[error("value not of type String")]
    NotString,

    #[error("Property {0} not found ")]
    PropertyNotFound(String),

    #[error("failed to create event")]
    EventNotReturned,

    #[error("event not found")]
    EventNotFound,
}

#[derive(Deserialize, Serialize)]
struct AlertDB {
    r#type: String,
    follower: String,
}

pub struct Store {
    db: Surreal<Db>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DbEvent {
    alert_type: AlertEventType,
    ctime: Datetime,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DbEventQuery {
    pub id: RecordId,
    pub alert_type: AlertEventType,
    pub ctime: surrealdb::Datetime,
}

impl Store {
    pub async fn new() -> Result<Self, DatabaseError> {
        let db = Surreal::new::<RocksDb>(&APP.config.database_path).await?;

        db.use_ns("activity_feed").use_db("events").await?;

        Ok(Self { db })
    }

    pub async fn new_event(
        &self,
        alert: AlertEventType,
    ) -> Result<Option<DbEventQuery>, DatabaseError> {
        let now = Datetime::default();

        Ok(self
            .db
            .create("events")
            .content(DbEvent {
                alert_type: alert,
                ctime: now,
            })
            .await?)
    }

    pub async fn get_events(&self) -> Result<Vec<DbEventQuery>, DatabaseError> {
        Ok(self.db.select("events").await?)
    }

    pub async fn get_event(&self, id: RecordIdKey) -> Result<Option<DbEventQuery>, DatabaseError> {
        Ok(self.db.select(("events", id)).await?)
    }

    pub async fn delete_events_table(&self) -> Result<Vec<DbEventQuery>, DatabaseError> {
        Ok(self.db.delete("events").await?)
    }
}

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{APP, AlertEventType, OneShotSender};

#[derive(thiserror::Error, Debug)]
pub enum DatabaseError {
    #[error(transparent)]
    SQLiteError(#[from] rusqlite::Error),

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

pub enum DBMessage {
    NewEvent(AlertEventType),
    GetEvent(i32, OneShotSender<DbEventRecord>),
    GetEvents(OneShotSender<Vec<DbEventRecord>>),
}

pub struct Store {
    db: Connection,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DbEvent {
    alert_type: AlertEventType,
    ctime: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DbEventRecord {
    pub id: i32,
    pub alert_type: AlertEventType,
    pub ctime: DateTime<Utc>,
}

impl Store {
    pub fn new() -> Result<Self, DatabaseError> {
        let db = Connection::open(&APP.config.database_path)?;

        db.execute(
            r#"
                CREATE TABLE IF NOT EXISTS events (
                    id INTEGER PRIMARY KEY,
                    data TEXT NOT NULL
                );
            "#,
            (),
        )?;

        Ok(Self { db })
    }

    pub fn new_event(&self, alert: AlertEventType) -> Result<DbEventRecord, DatabaseError> {
        let now = Utc::now();

        Ok(self.db.query_one(
            r#"
                INSERT INTO events (data) VALUES (?1) RETURNING id, data
            "#,
            (serde_json::to_string(&DbEvent {
                alert_type: alert,
                ctime: now,
            })
            .unwrap(),),
            |row| {
                let event = serde_json::from_str::<DbEvent>(&row.get::<_, String>(1)?).unwrap();
                Ok(DbEventRecord {
                    id: row.get(0)?,
                    alert_type: event.alert_type,
                    ctime: event.ctime,
                })
            },
        )?)
    }

    pub fn get_events(&self) -> Result<Vec<DbEventRecord>, DatabaseError> {
        let mut stmt = self.db.prepare("SELECT * FROM events")?;

        let res: Result<Vec<DbEventRecord>, rusqlite::Error> = stmt
            .query_map((), |row| {
                let id = row.get(0)?;
                let event = serde_json::from_str::<DbEvent>(&row.get::<_, String>(1)?).unwrap();
                Ok(DbEventRecord {
                    id,
                    alert_type: event.alert_type,
                    ctime: event.ctime,
                })
            })?
            .collect();

        Ok(res?)
    }

    pub fn get_event(&self, id: i32) -> Result<Option<DbEventRecord>, DatabaseError> {
        Ok(self
            .db
            .query_one(r#"SELECT * from events WHERE id = ?1"#, (id,), |row| {
                let id = row.get(0)?;
                let event = serde_json::from_str::<DbEvent>(&row.get::<_, String>(1)?).unwrap();
                Ok(DbEventRecord {
                    id,
                    alert_type: event.alert_type,
                    ctime: event.ctime,
                })
            })
            .optional()?)
    }
}

use std::{collections::BTreeMap, convert::TryInto};

use serde::{Deserialize, Serialize};
use surrealdb::{
    sql::{Array, Datetime, Object, Value},
    Datastore, Session,
};

use crate::{collection, AlertEventType, W};

#[derive(Deserialize, Serialize)]
struct AlertDB {
    r#type: String,
    follower: String,
}

pub struct Store {
    ds: Datastore,
    session: Session,
}

impl Store {
    pub async fn new() -> eyre::Result<Self> {
        let ds = Datastore::new("file://database.db").await?;
        let session = Session::for_db("activity_feed", "events");

        Ok(Self { ds, session })
    }

    pub async fn new_event(&self, alert: AlertEventType) -> eyre::Result<()> {
        let sql = "CREATE events CONTENT $data RETURN id";

        // let mut data: BTreeMap<String, Value> = collection! {
        //     "data".into() => ,
        // };

        let mut data: Object = W(alert.into()).try_into()?;

        let now = Datetime::default().timestamp_nanos();

        data.insert("ctime".into(), now.into());

        let vars: BTreeMap<String, Value> = collection! {
            "data".into() => data.into()
        };

        let res = self
            .ds
            .execute(sql, &self.session, Some(vars), false)
            .await?;

        let first_val = res
            .into_iter()
            .next()
            .map(|r| r.result)
            .expect("id not returned")?;

        if let Value::Object(val) = first_val.first() {
            println!(
                "{:#?}",
                val.get("id").expect("value should have an id").to_string()
            );
        } else {
            return Err(eyre::eyre!("event creation returned nothing"));
        }

        Ok(())
    }

    pub async fn get_events(&self) -> eyre::Result<Vec<Object>> {
        let sql = "SELECT * from events";

        let res = self.ds.execute(sql, &self.session, None, false).await?;

        let res = res.into_iter().next().expect("no response");

        let arr: Array = W(res.result?).try_into()?;

        arr.into_iter().map(|value| W(value).try_into()).collect()
    }

    pub async fn delete_events_table(&self) -> eyre::Result<()> {
        let sql = "DELETE events";

        let res = self.ds.execute(sql, &self.session, None, false).await?;

        let res = res.into_iter().next().expect("Did not get a response");

        res.result?;

        Ok(())
    }
}

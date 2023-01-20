use std::{collections::BTreeMap, convert::TryInto};

use serde::{Deserialize, Serialize};
use surrealdb::{
    sql::{Array, Datetime, Object, Value},
    Datastore, Session,
};

use crate::{collection, AlertEventType, TakeVal, Wrapper};

// TODO: use stuff like this for database data
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

    pub async fn new_event(&self, alert: AlertEventType) -> eyre::Result<String> {
        let sql = "CREATE events CONTENT $data RETURN id";

        let mut data: Object = Wrapper(alert.into()).try_into()?;

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

        if let Value::Object(mut val) = first_val.first() {
            val.take_val::<String>("id")
        } else {
            return Err(eyre::eyre!("event creation returned nothing"));
        }
    }

    pub async fn get_events(&self) -> eyre::Result<Vec<Object>> {
        let sql = "SELECT * from events";

        let res = self.ds.execute(sql, &self.session, None, false).await?;

        let res = res.into_iter().next().expect("no response");

        let arr: Array = Wrapper(res.result?).try_into()?;

        arr.into_iter()
            .map(|value| Wrapper(value).try_into())
            .collect()
    }

    pub async fn get_event(&self, id: &str) -> eyre::Result<Object> {
        let sql = format!("SELECT * FROM {}", id);

        // let mut vars: BTreeMap<String, Value> = collection! {};

        // let obj: Object = Wrapper(filter).try_into()?;
        // sql.push_str(" WHERE");
        // for (idx, (k, v)) in obj.into_iter().enumerate() {
        //     // SELECT * FROM events WHERE id = $w0
        //     // "w0" => "{v}"

        //     let var = format!("w{idx}");
        //     sql.push_str(&format!(" {k} = ${var}"));
        //     vars.insert(var, v);
        // }

        let res = self.ds.execute(&sql, &self.session, None, false).await?;

        let first_val = res
            .into_iter()
            .next()
            .map(|r| r.result)
            .expect("no response")?;

        if let Value::Object(val) = first_val.first() {
            Ok(val)
        } else {
            return Err(eyre::eyre!("Event not found"));
        }
    }

    pub async fn delete_events_table(&self) -> eyre::Result<()> {
        let sql = "DELETE events";

        let res = self.ds.execute(sql, &self.session, None, false).await?;

        let res = res.into_iter().next().expect("Did not get a response");

        res.result?;

        println!("Deleted events table");

        Ok(())
    }

    pub async fn rename_field(
        &self,
        field_name: String,
        new_field_name: String,
    ) -> eyre::Result<()> {
        let sql = format!("UPDATE events SET {} = {}", new_field_name, field_name);
        self.ds.execute(&sql, &self.session, None, false).await?;

        let sql = format!("UPDATE events SET {} = NONE;", field_name);
        let res = self.ds.execute(&sql, &self.session, None, false).await?;
        println!("{:#?}", res);

        Ok(())
    }

    pub async fn capitalize_value(&self) -> eyre::Result<()> {
        let sql = String::from(r#"UPDATE events SET type = "Raid" WHERE type = "raid" RETURN id;"#);
        let res = self.ds.execute(&sql, &self.session, None, false).await?;
        println!("{:#?}", res);

        Ok(())
    }
}

//! SQLite implementation of [`MachineRepository`].

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mcpmux_core::{Machine, MachineRepository};
use rusqlite::{params, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::Database;

/// SQLite-backed implementation of [`MachineRepository`].
pub struct SqliteMachineRepository {
    db: Arc<Mutex<Database>>,
}

impl SqliteMachineRepository {
    /// Create a new SQLite machine repository.
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    /// Parse a datetime string to `DateTime<Utc>`.
    fn parse_datetime(s: &str) -> DateTime<Utc> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return dt.with_timezone(&Utc);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return dt.and_utc();
        }
        Utc::now()
    }

    const COLUMNS: &'static str = "id, name, icon, hostname, created_at, updated_at";

    fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Machine> {
        let id_str: String = row.get(0)?;
        Ok(Machine {
            id: id_str.parse().unwrap_or_else(|_| Uuid::new_v4()),
            name: row.get(1)?,
            icon: row.get(2)?,
            hostname: row.get(3)?,
            created_at: Self::parse_datetime(&row.get::<_, String>(4)?),
            updated_at: Self::parse_datetime(&row.get::<_, String>(5)?),
        })
    }
}

#[async_trait]
impl MachineRepository for SqliteMachineRepository {
    async fn list(&self) -> Result<Vec<Machine>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let sql = format!("SELECT {} FROM machines ORDER BY name ASC", Self::COLUMNS);
        let mut stmt = conn.prepare(&sql)?;
        let machines = stmt
            .query_map([], Self::map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(machines)
    }

    async fn get(&self, id: &Uuid) -> Result<Option<Machine>> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let sql = format!("SELECT {} FROM machines WHERE id = ?", Self::COLUMNS);
        let mut stmt = conn.prepare(&sql)?;
        let machine = stmt
            .query_row(params![id.to_string()], Self::map_row)
            .optional()?;
        Ok(machine)
    }

    async fn create(&self, machine: &Machine) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute(
            "INSERT INTO machines (id, name, icon, hostname, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                machine.id.to_string(),
                machine.name,
                machine.icon,
                machine.hostname,
                machine.created_at.to_rfc3339(),
                machine.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    async fn update(&self, machine: &Machine) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        let rows = conn.execute(
            "UPDATE machines
             SET name = ?2, icon = ?3, hostname = ?4, updated_at = ?5
             WHERE id = ?1",
            params![
                machine.id.to_string(),
                machine.name,
                machine.icon,
                machine.hostname,
                machine.updated_at.to_rfc3339(),
            ],
        )?;
        if rows == 0 {
            anyhow::bail!("Machine not found: {}", machine.id);
        }
        Ok(())
    }

    async fn delete(&self, id: &Uuid) -> Result<()> {
        let db = self.db.lock().await;
        let conn = db.connection();
        conn.execute("DELETE FROM machines WHERE id = ?", params![id.to_string()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpmux_core::MachineRepository;

    #[tokio::test]
    async fn test_machine_crud_round_trip() {
        let db = Arc::new(Mutex::new(Database::open_in_memory().unwrap()));
        let repo = SqliteMachineRepository::new(db);

        let mut machine = Machine::new("Gondor");
        machine.icon = Some("🖥️".to_string());
        machine.hostname = Some("gondor.local".to_string());
        repo.create(&machine).await.unwrap();

        let listed = repo.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Gondor");
        assert_eq!(listed[0].icon.as_deref(), Some("🖥️"));

        let got = repo.get(&machine.id).await.unwrap().unwrap();
        assert_eq!(got.hostname.as_deref(), Some("gondor.local"));

        let mut updated = got;
        updated.name = "Box 1".to_string();
        updated.updated_at = Utc::now();
        repo.update(&updated).await.unwrap();

        let after = repo.get(&machine.id).await.unwrap().unwrap();
        assert_eq!(after.name, "Box 1");

        repo.delete(&machine.id).await.unwrap();
        assert!(repo.get(&machine.id).await.unwrap().is_none());
    }
}

//! Local heart-rate history in SQLite (`history.db` in the app data
//! directory). Only real strap data is recorded, as 5-second averages, and
//! anything older than a week is deleted. Nothing leaves the machine.

use rusqlite::{params, Connection};
use serde::Serialize;
use std::{fs, path::Path, sync::Mutex};

pub const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// Day queries are bucketed to one point per minute.
const BUCKET_MS: i64 = 60_000;

pub struct Db(Mutex<Connection>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub t: i64,
    pub avg: f64,
    pub min: u16,
    pub max: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub start: i64,
    pub end: i64,
    pub peak: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayHistory {
    pub points: Vec<Point>,
    pub episodes: Vec<Episode>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS samples (t INTEGER PRIMARY KEY, bpm INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS episodes (
                 start INTEGER PRIMARY KEY, end INTEGER NOT NULL, peak INTEGER NOT NULL
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self(Mutex::new(conn)))
    }

    pub fn add_sample(&self, t: i64, bpm: u16) {
        let _ = self.0.lock().unwrap().execute(
            "INSERT OR REPLACE INTO samples (t, bpm) VALUES (?1, ?2)",
            params![t, bpm],
        );
    }

    pub fn add_episode(&self, start: i64, end: i64, peak: u16) {
        let _ = self.0.lock().unwrap().execute(
            "INSERT OR REPLACE INTO episodes (start, end, peak) VALUES (?1, ?2, ?3)",
            params![start, end, peak],
        );
    }

    /// Delete everything older than the retention window.
    pub fn prune(&self, now: i64) {
        let cutoff = now - RETENTION_MS;
        let conn = self.0.lock().unwrap();
        let _ = conn.execute("DELETE FROM samples WHERE t < ?1", params![cutoff]);
        let _ = conn.execute("DELETE FROM episodes WHERE start < ?1", params![cutoff]);
    }

    pub fn range(&self, from: i64, to: i64) -> Result<DayHistory, String> {
        let conn = self.0.lock().unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT (t / ?3) * ?3, AVG(bpm), MIN(bpm), MAX(bpm) FROM samples
                 WHERE t >= ?1 AND t < ?2 GROUP BY t / ?3 ORDER BY 1",
            )
            .map_err(|e| e.to_string())?;
        let points = stmt
            .query_map(params![from, to, BUCKET_MS], |r| {
                Ok(Point { t: r.get(0)?, avg: r.get(1)?, min: r.get(2)?, max: r.get(3)? })
            })
            .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            .map_err(|e| e.to_string())?;

        let mut stmt = conn
            .prepare("SELECT start, end, peak FROM episodes WHERE start >= ?1 AND start < ?2 ORDER BY start DESC")
            .map_err(|e| e.to_string())?;
        let episodes = stmt
            .query_map(params![from, to], |r| {
                Ok(Episode { start: r.get(0)?, end: r.get(1)?, peak: r.get(2)? })
            })
            .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            .map_err(|e| e.to_string())?;

        Ok(DayHistory { points, episodes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_by_minute_and_prunes_after_a_week() {
        let dir = std::env::temp_dir().join(format!("pulsebar-test-{}", std::process::id()));
        let db = Db::open(&dir.join("history.db")).unwrap();
        let now = 1_800_000_000_000_i64;

        db.add_sample(now, 60);
        db.add_sample(now + 5_000, 70);
        db.add_sample(now + 65_000, 90);
        db.add_sample(now - RETENTION_MS - 1, 55);
        db.add_episode(now, now + 40_000, 96);
        db.add_episode(now - RETENTION_MS - 1, now - RETENTION_MS, 99);

        db.prune(now);
        let day = db.range(now - RETENTION_MS * 2, now + 120_000).unwrap();
        assert_eq!(day.points.len(), 2);
        assert_eq!((day.points[0].avg, day.points[0].min, day.points[0].max), (65.0, 60, 70));
        assert_eq!(day.points[1].max, 90);
        assert_eq!(day.episodes.len(), 1);
        assert_eq!(day.episodes[0].peak, 96);

        let _ = fs::remove_dir_all(dir);
    }
}

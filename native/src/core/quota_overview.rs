//! A read-only, aggregated view of every account's quota windows.
//!
//! The account list already shows a reset badge per account, but there is no
//! single place that answers "which of my subscriptions renews soon with quota
//! left?". This module reads the quota columns the refresh service already
//! writes; it never contacts an upstream, so opening the view is cheap and
//! cannot invalidate a session.

use crate::error::AppError;
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// One account's quota as last refreshed by the route/quota service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaOverviewRow {
    pub id: String,
    pub display_name: String,
    pub provider: String,
    pub subscription_type: Option<String>,
    pub primary_remain: Option<i64>,
    pub weekly_remain: Option<i64>,
    pub reset_primary: Option<String>,
    pub reset_weekly: Option<String>,
    pub quota_limit: Option<i64>,
    pub quota_used: Option<i64>,
    pub quota_updated_at: Option<String>,
}

/// A renewal warning: a window ends soon while quota is still unspent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaReminder {
    pub credential_id: String,
    pub display_name: String,
    /// `primary` or `weekly`, matching the two windows the proxy tracks.
    pub window: String,
    pub reset_at: String,
    pub minutes_left: i64,
    pub remaining: i64,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaOverview {
    pub rows: Vec<QuotaOverviewRow>,
    pub reminders: Vec<QuotaReminder>,
    pub official_account_count: i64,
    pub with_quota_count: i64,
}

/// A window renewing inside a day, with at least a fifth of it still unspent,
/// is worth warning about: that quota is lost at reset unless it is used.
const REMINDER_WINDOW_MINUTES: i64 = 24 * 60;
const LOW_SPEND_PERCENT: i64 = 20;

fn parse_reset(value: Option<&str>) -> Option<i64> {
    DateTime::parse_from_rfc3339(value?.trim())
        .ok()
        .map(|parsed| parsed.timestamp_millis())
}

fn remaining_is_unspent(row: &QuotaOverviewRow, remaining: i64) -> bool {
    if remaining <= 0 {
        return false;
    }
    match row.quota_limit {
        Some(limit) if limit > 0 => remaining.saturating_mul(100) >= limit.saturating_mul(LOW_SPEND_PERCENT),
        // Without a limit there is no basis for calling the remainder "a lot";
        // a positive balance is still worth showing as renewing.
        _ => true,
    }
}

/// Build the reminders from the same rows the view displays, so a stale row
/// never produces a warning about a quota the app no longer holds.
fn reminders_for(rows: &[QuotaOverviewRow], now_ms: i64) -> Vec<QuotaReminder> {
    let mut reminders = Vec::new();
    for row in rows {
        for (window, reset, remaining) in [
            ("primary", row.reset_primary.as_deref(), row.primary_remain),
            ("weekly", row.reset_weekly.as_deref(), row.weekly_remain),
        ] {
            let (Some(reset), Some(remaining)) = (reset, remaining) else {
                continue;
            };
            if !remaining_is_unspent(row, remaining) {
                continue;
            }
            let Some(reset_ms) = parse_reset(Some(reset)) else {
                continue;
            };
            let minutes_left = (reset_ms - now_ms) / 60_000;
            if minutes_left < 0 || minutes_left > REMINDER_WINDOW_MINUTES {
                continue;
            }
            let label = if window == "primary" { "5h" } else { "周" };
            reminders.push(QuotaReminder {
                credential_id: row.id.clone(),
                display_name: row.display_name.clone(),
                window: window.to_string(),
                reset_at: reset.to_string(),
                minutes_left,
                remaining,
                message: format!(
                    "{label} 窗口约 {} 分钟后续期，仍有 {remaining} 未使用，续期后可能清零",
                    minutes_left.max(0)
                ),
            });
        }
    }
    reminders.sort_by_key(|item| item.minutes_left);
    reminders
}

/// Read all official accounts' quota snapshots and derive renewal reminders.
pub async fn get_quota_overview_core(pool: &SqlitePool) -> Result<QuotaOverview, AppError> {
    let rows = sqlx::query(
        "SELECT rc.id, rc.display_name, rc.provider, rc.subscription_type,
                rc.primary_remain, rc.weekly_remain, rc.reset_primary, rc.reset_weekly,
                rc.quota_limit, rc.quota_used, rc.quota_updated_at
         FROM route_credentials rc
         WHERE rc.archived_at IS NULL AND rc.kind = 'official'
         ORDER BY rc.reset_primary ASC, rc.reset_weekly ASC, rc.sort_order ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| AppError::Database {
        code: "database.quota_overview",
        message: "Could not load account quotas".to_string(),
        details: Some(error.to_string()),
        recoverable: true,
    })?;

    let rows: Vec<QuotaOverviewRow> = rows
        .into_iter()
        .map(|row| QuotaOverviewRow {
            id: row.get("id"),
            display_name: row.get("display_name"),
            provider: row.get("provider"),
            subscription_type: row.get("subscription_type"),
            primary_remain: row.get("primary_remain"),
            weekly_remain: row.get("weekly_remain"),
            reset_primary: row.get("reset_primary"),
            reset_weekly: row.get("reset_weekly"),
            quota_limit: row.get("quota_limit"),
            quota_used: row.get("quota_used"),
            quota_updated_at: row.get("quota_updated_at"),
        })
        .collect();

    let official_account_count = rows.len() as i64;
    let with_quota_count = rows
        .iter()
        .filter(|row| row.primary_remain.is_some() || row.weekly_remain.is_some())
        .count() as i64;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let reminders = reminders_for(&rows, now_ms);

    Ok(QuotaOverview {
        rows,
        reminders,
        official_account_count,
        with_quota_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(reset_primary: Option<&str>, remaining: Option<i64>, limit: Option<i64>) -> QuotaOverviewRow {
        QuotaOverviewRow {
            id: "cred-1".to_string(),
            display_name: "Team".to_string(),
            provider: "openai".to_string(),
            subscription_type: None,
            primary_remain: remaining,
            weekly_remain: None,
            reset_primary: reset_primary.map(str::to_string),
            reset_weekly: None,
            quota_limit: limit,
            quota_used: None,
            quota_updated_at: None,
        }
    }

    #[test]
    fn a_soon_reset_with_unspent_quota_warns() {
        let now = DateTime::parse_from_rfc3339("2026-11-26T10:00:00Z")
            .unwrap()
            .timestamp_millis();
        let reset = "2026-11-26T12:00:00Z";
        let rows = vec![row(Some(reset), Some(400), Some(1000))];
        let reminders = reminders_for(&rows, now);
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].minutes_left, 120);
        assert!(reminders[0].message.contains("400"));
    }

    #[test]
    fn a_far_reset_or_a_near_empty_window_does_not_warn() {
        let now = DateTime::parse_from_rfc3339("2026-11-26T10:00:00Z")
            .unwrap()
            .timestamp_millis();
        assert!(reminders_for(
            &[row(Some("2026-11-28T12:00:00Z"), Some(400), Some(1000))],
            now,
        )
        .is_empty());
        assert!(reminders_for(
            &[row(Some("2026-11-26T12:00:00Z"), Some(10), Some(1000))],
            now,
        )
        .is_empty());
        assert!(reminders_for(
            &[row(Some("2026-11-26T09:00:00Z"), Some(400), Some(1000))],
            now,
        )
        .is_empty());
        assert!(reminders_for(&[row(None, Some(400), Some(1000))], now).is_empty());
    }
}

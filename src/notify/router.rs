use crate::core::{
    AlertState, AppConfig, Notification, NotificationPrefs, ProjectId, UpdateSeverity,
};
use chrono::{Local, Timelike, Utc};
use notify_rust::Notification as DbusNotification;
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Routes notifications based on severity, quiet hours, batching, and per-project overrides.
pub struct NotificationRouter {
    config: AppConfig,
    pending: Mutex<Vec<Notification>>,
    sent_ids: Mutex<HashMap<(ProjectId, String, String), AlertState>>,
}

impl NotificationRouter {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            pending: Mutex::new(Vec::new()),
            sent_ids: Mutex::new(HashMap::new()),
        }
    }

    /// Queue a notification for potential delivery.
    /// Applies severity filtering, quiet hours, and deduplication.
    pub fn queue(
        &self,
        project_id: ProjectId,
        severity: UpdateSeverity,
        title: String,
        body: String,
    ) {
        let prefs = self.effective_prefs(project_id);

        if !prefs.enabled {
            debug!("Notifications disabled for project {:?}", project_id);
            return;
        }

        if !self.meets_severity_threshold(severity, &prefs) {
            debug!(
                "Severity {:?} below threshold for project {:?}",
                severity, project_id
            );
            return;
        }
        match severity {
            UpdateSeverity::Minor if !prefs.show_minor_updates => return,
            UpdateSeverity::Major if !prefs.show_major_updates => return,
            UpdateSeverity::Security if !prefs.show_security => return,
            _ => {}
        }
        if self.pending.lock().unwrap().iter().any(|notification| {
            notification.project_id == project_id
                && notification.title == title
                && notification.body == body
        }) || self.sent_ids.lock().unwrap().contains_key(&(
            project_id,
            title.clone(),
            body.clone(),
        )) {
            return;
        }

        if self.is_quiet_hours(&prefs) {
            debug!("Quiet hours active, deferring notification: {}", title);
            // Still queue it — will be flushed when quiet hours end
        }

        let notification = Notification {
            id: Uuid::new_v4(),
            project_id,
            severity,
            title,
            body,
            state: AlertState::Active,
            created_at: Utc::now(),
            sent_at: None,
            acknowledged_at: None,
        };

        let mut pending = self.pending.lock().unwrap();
        pending.push(notification);
    }

    /// Flush pending notifications that are ready to send.
    /// Respects batch intervals and quiet hours.
    pub fn flush(&self) {
        let mut pending = self.pending.lock().unwrap();
        let mut sent_ids = self.sent_ids.lock().unwrap();

        let now = Utc::now();
        let global_prefs = &self.config.global_notification_prefs;

        if self.is_quiet_hours(global_prefs) {
            debug!("Quiet hours active, skipping flush");
            return;
        }

        let mut to_send = Vec::new();
        let mut remaining = Vec::new();

        for notif in pending.drain(..) {
            let age_secs = (now - notif.created_at).num_seconds() as u64;
            let batch_interval = self.batch_interval_for(notif.project_id);

            // Security updates bypass batching
            if notif.severity == UpdateSeverity::Security || age_secs >= batch_interval {
                to_send.push(notif);
            } else {
                remaining.push(notif);
            }
        }

        *pending = remaining;

        // Batch low-severity into digest
        let (high_priority, low_priority): (Vec<_>, Vec<_>) = to_send
            .into_iter()
            .partition(|n| n.severity == UpdateSeverity::Security);

        // Send high-priority individually
        for notif in &high_priority {
            if let Err(e) = self.send_dbus(notif) {
                warn!("Failed to send notification '{}': {}", notif.title, e);
            } else {
                sent_ids.insert(
                    (notif.project_id, notif.title.clone(), notif.body.clone()),
                    AlertState::Active,
                );
                info!("Sent notification: {} ({:?})", notif.title, notif.severity);
            }
        }

        // Batch low-priority into digest
        if !low_priority.is_empty() {
            let digest_title = format!(" {} dependency updates available", low_priority.len());
            let digest_body: String = low_priority
                .iter()
                .take(10) // Cap digest at 10 items
                .map(|n| {
                    format!(
                        "• {}: {} → {}",
                        n.title,
                        n.body.split("→").next().unwrap_or("?").trim(),
                        n.body.split("→").nth(1).unwrap_or("?").trim()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            let digest_notif = Notification {
                id: Uuid::new_v4(),
                project_id: low_priority[0].project_id,
                severity: UpdateSeverity::Minor,
                title: digest_title,
                body: digest_body,
                state: AlertState::Active,
                created_at: now,
                sent_at: Some(now),
                acknowledged_at: None,
            };

            if let Err(e) = self.send_dbus(&digest_notif) {
                warn!("Failed to send digest notification: {}", e);
            } else {
                for notification in &low_priority {
                    sent_ids.insert(
                        (
                            notification.project_id,
                            notification.title.clone(),
                            notification.body.clone(),
                        ),
                        AlertState::Active,
                    );
                }
                info!("Sent digest with {} updates", low_priority.len());
            }
        }
    }

    fn send_dbus(&self, notif: &Notification) -> Result<(), notify_rust::error::Error> {
        let urgency = match notif.severity {
            UpdateSeverity::Security => notify_rust::Urgency::Critical,
            UpdateSeverity::Major => notify_rust::Urgency::Normal,
            UpdateSeverity::Minor => notify_rust::Urgency::Low,
            UpdateSeverity::Info => notify_rust::Urgency::Low,
        };

        DbusNotification::new()
            .summary(&notif.title)
            .body(&notif.body)
            .urgency(urgency)
            .appname("appchecker")
            .show()?;

        Ok(())
    }

    fn effective_prefs(&self, project_id: ProjectId) -> NotificationPrefs {
        self.config
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .and_then(|p| p.notification_overrides.clone())
            .unwrap_or_else(|| self.config.global_notification_prefs.clone())
    }

    fn meets_severity_threshold(
        &self,
        severity: UpdateSeverity,
        prefs: &NotificationPrefs,
    ) -> bool {
        severity >= prefs.min_severity
    }

    fn is_quiet_hours(&self, prefs: &NotificationPrefs) -> bool {
        let (start, end) = match (&prefs.quiet_hours_start, &prefs.quiet_hours_end) {
            (Some(s), Some(e)) => (s, e),
            _ => return false,
        };

        let now = Local::now();
        let current_mins = now.hour() * 60 + now.minute();

        let start_mins = Self::parse_time_mins(start);
        let end_mins = Self::parse_time_mins(end);

        if start_mins <= end_mins {
            current_mins >= start_mins && current_mins < end_mins
        } else {
            // Wraps midnight: e.g., 22:00 → 08:00
            current_mins >= start_mins || current_mins < end_mins
        }
    }

    fn parse_time_mins(time_str: &str) -> u32 {
        let parts: Vec<&str> = time_str.split(':').collect();
        if parts.len() == 2 {
            let h: u32 = parts[0].parse().unwrap_or(0);
            let m: u32 = parts[1].parse().unwrap_or(0);
            h * 60 + m
        } else {
            0
        }
    }

    fn batch_interval_for(&self, project_id: ProjectId) -> u64 {
        self.config
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .and_then(|p| p.notification_overrides.as_ref())
            .map(|o| o.batch_interval_secs)
            .unwrap_or(self.config.global_notification_prefs.batch_interval_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Project;

    #[test]
    fn queue_honors_visibility_preferences_and_deduplicates_updates() {
        let project = Project::new("demo".into(), std::path::PathBuf::from("/tmp/demo"));
        let config = AppConfig {
            projects: vec![project.clone()],
            ..AppConfig::default()
        };
        let router = NotificationRouter::new(config);
        let title = "serde update available".to_string();
        let body = "serde 1.0.0 → 2.0.0".to_string();

        router.queue(
            project.id,
            UpdateSeverity::Minor,
            title.clone(),
            body.clone(),
        );
        assert!(router.pending.lock().unwrap().is_empty());
        router.queue(
            project.id,
            UpdateSeverity::Major,
            title.clone(),
            body.clone(),
        );
        router.queue(
            project.id,
            UpdateSeverity::Major,
            title.clone(),
            body.clone(),
        );
        assert_eq!(router.pending.lock().unwrap().len(), 1);

        let key = (project.id, title.clone(), body.clone());
        router
            .sent_ids
            .lock()
            .unwrap()
            .insert(key, AlertState::Active);
        router.queue(project.id, UpdateSeverity::Major, title, body);
        assert_eq!(router.pending.lock().unwrap().len(), 1);
    }
}

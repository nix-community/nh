use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
    thread,
};

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    fn get_capabilities(&self) -> zbus::Result<Vec<String>>;

    #[zbus(signal)]
    fn action_invoked(id: u32, action_key: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn notification_closed(id: u32, reason: u32) -> zbus::Result<()>;
}

/// Urgency level for notifications, affecting how they are displayed.
///
/// Different urgency levels may be rendered differently by notification daemons,
/// such as using different colors, sounds, or persistence behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// Low urgency - typically for background information
    Low = 0,
    /// Normal urgency - the default for most notifications
    Normal = 1,
    /// Critical urgency - for important alerts that require attention
    Critical = 2,
}

#[derive(Debug, thiserror::Error)]
pub enum NotificationError {
    #[error("Failed to connect to notification daemon: {0}")]
    Connection(#[from] zbus::Error),
    #[error("Failed to receive notification response: {0}")]
    Response(#[from] std::sync::mpsc::RecvError),
}

/// Response received from a notification interaction.
#[derive(Debug, Clone)]
pub enum NotificationResponse {
    /// User clicked on an action button
    Action(Arc<str>),
    /// Notification was dismissed without action
    Dismissed,
}

pub struct NoSummary;
pub struct WithSummary;

pub struct NotificationBuilder<'a, State = NoSummary> {
    summary: &'a str,
    body: &'a str,
    icon: &'a str,
    actions: Vec<Action>,
    urgency: Urgency,
    _state: std::marker::PhantomData<State>,
}

pub struct Action {
    pub key: Arc<str>,
    pub name: Box<str>,
}

/// Creates a new notification builder.
///
/// This is the entry point for building notifications. You must call
/// `with_summary()` before the notification can be sent.
///
/// # Example
/// ```no_run
/// use nh::notify::{notify, Urgency};
///
/// let response = notify()
///     .with_summary("nh os switch")
///     .with_body("NixOS configuration built successfully.")
///     .with_urgency(Urgency::Critical)
///     .with_action("default", "Apply")
///     .with_action("reject", "Reject")
///     .send()
///     .unwrap();
/// ```
#[must_use]
pub fn notify<'a>() -> NotificationBuilder<'a, NoSummary> {
    NotificationBuilder {
        body: "",
        summary: "",
        icon: "nix-snowflake",
        urgency: Urgency::Normal,
        actions: Vec::new(),
        _state: std::marker::PhantomData,
    }
}

impl<'a> NotificationBuilder<'a, NoSummary> {
    /// Sets the notification summary (required).
    #[must_use]
    pub fn with_summary(self, summary: &'a str) -> NotificationBuilder<'a, WithSummary> {
        NotificationBuilder {
            summary,
            body: self.body,
            icon: self.icon,
            urgency: self.urgency,
            actions: self.actions,
            _state: std::marker::PhantomData,
        }
    }
}

impl<'a, S> NotificationBuilder<'a, S> {
    /// Sets the notification body text.
    #[must_use]
    pub fn with_body(mut self, body: &'a str) -> Self {
        self.body = body;
        self
    }

    /// Sets the urgency level.
    #[must_use]
    pub fn with_urgency(mut self, urgency: Urgency) -> Self {
        self.urgency = urgency;
        self
    }

    /// Adds a clickable action button.
    #[must_use]
    pub fn with_action<T, U>(mut self, key: T, name: U) -> Self
    where
        T: Into<Arc<str>>,
        U: Into<Box<str>>,
    {
        self.actions.push(Action {
            key: key.into(),
            name: name.into(),
        });
        self
    }
}

impl NotificationBuilder<'_, WithSummary> {
    /// Sends the notification. Returns `None` if no actions or actions unsupported.
    /// Otherwise blocks until user interaction.
    #[must_use]
    pub fn send(self) -> Result<Option<NotificationResponse>, NotificationError> {
        let conn = zbus::blocking::Connection::session()?;
        let proxy = NotificationsProxyBlocking::new(&conn)?;

        let capabilities = proxy.get_capabilities()?;

        let mut hints = HashMap::new();
        hints.insert("urgency", zbus::zvariant::Value::U8(self.urgency as u8));

        // The D-Bus notification spec expects the `actions` field to be a list of strings
        let flattened_actions: Vec<_> = self
            .actions
            .iter()
            .flat_map(|action| [&*action.key, &*action.name])
            .collect();

        let id = proxy.notify(
            "nh",
            0, // Notification ID (0 = request new ID from server)
            self.icon,
            self.summary,
            self.body,
            &flattened_actions,
            hints,
            -1, // Timeout in milliseconds (-1 = server default)
        )?;

        if !capabilities.iter().any(|cap| *cap == "actions") || self.actions.is_empty() {
            return Ok(None);
        }

        let (tx, rx) = mpsc::channel();

        {
            let tx = tx.clone();
            let mut notification_closed_stream = proxy.receive_notification_closed()?;
            // Spawn a separate thread to listen for the 'NotificationClosed' signal
            thread::spawn(move || {
                while let Some(notification_closed) = notification_closed_stream.next() {
                    if let Ok(args) = notification_closed.args() {
                        if args.id == id {
                            let _ = tx.send(NotificationResponse::Dismissed);
                            break;
                        }
                    }
                }
            });
        }

        {
            let mut action_invoked_stream = proxy.receive_action_invoked()?;
            // Spawn a separate thread to listen for the 'ActionInvoked' signal
            thread::spawn(move || {
                while let Some(action_invoked) = action_invoked_stream.next() {
                    if let Ok(args) = action_invoked.args() {
                        if args.id == id {
                            if let Some(action) = self
                                .actions
                                .iter()
                                .find(|action| &*action.key == args.action_key)
                            {
                                _ = tx.send(NotificationResponse::Action(Arc::clone(&action.key)));
                                break;
                            }
                        }
                    }
                }
            });
        }

        let result = rx.recv().ok();
        Ok(result)
    }
}

use std::env;

use color_eyre::eyre::Result;
use notify_rust::{Notification, Urgency};

/// A thin wrapper around [`notify_rust::Notification`]
#[derive(Debug)]
pub struct NotificationSender {
  notification: notify_rust::Notification,
  urgency:      Option<Urgency>,
}

impl NotificationSender {
  /// Creates a new notification with a summary (title) and body (message).
  ///
  /// # Arguments
  ///
  /// * `summary` - Short title text displayed in the notification.
  /// * `body` - Longer description text shown below the summary.
  ///
  /// # Returns
  ///
  /// A [`NotificationSender`] containing a notification ready to be shown.
  #[must_use]
  pub fn new(summary: &str, body: &str) -> Self {
    let mut notification = Notification::new();
    notification.summary(summary);
    notification.body(body);
    Self {
      notification,
      urgency: None,
    }
  }

  /// Sets the urgency level of the notification.
  ///
  /// # Arguments
  ///
  /// * `urgency` - The desired [`Urgency`] for the notification.
  #[must_use]
  pub fn urgency(mut self, urgency: Urgency) -> Self {
    self.urgency = Some(urgency);
    self
  }

  /// Sends the notification to the desktop environment.
  ///
  /// Notifications will only be sent if the environment variable
  /// `NH_NOTIFY` is set to `"1"`. Otherwise, this function does nothing
  /// and returns `Ok(())`.
  ///
  /// On Unix platforms (excluding macOS), the urgency level is set to
  /// the value specified with [`NotificationSender::urgency`], or
  /// [`Urgency::Normal`] if none was set.
  ///
  /// # Errors
  ///
  /// Returns an error if the underlying notification system fails to
  /// display the message.
  pub fn send(mut self) -> Result<()> {
    let enable_notifications = env::var("NH_NOTIFY").is_ok_and(|v| v == "1");
    if !enable_notifications {
      return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    self
      .notification
      .urgency(self.urgency.unwrap_or(Urgency::Normal));

    self.notification.show()?;
    Ok(())
  }

  /// Shows a notification asking for user confirmation.
  ///
  /// On supported Unix platforms (excluding macOS), the notification will
  /// present **Accept** and **Reject** actions. The method blocks until
  /// the user selects an action, and returns:
  ///
  /// * `Ok(true)` if the user clicked **Accept**.
  /// * `Ok(false)` if the user clicked **Reject** or if actions are not
  ///   supported on the platform.
  ///
  /// The urgency level is set to the value specified with
  /// [`NotificationSender::urgency`], or [`Urgency::Critical`] if none was set.
  ///
  /// On unsupported platforms (e.g., macOS, Windows), this function always
  /// returns `Ok(false)` since interactive actions are not supported.
  ///
  /// # Errors
  ///
  /// Returns an error if the notification cannot be shown.
  pub fn ask(mut self) -> Result<bool> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
      self
        .notification
        .urgency(self.urgency.unwrap_or(Urgency::Critical));
      self.notification.action("accept", "Accept");
      self.notification.action("reject", "Reject");
    }

    let handle = self.notification.show().unwrap();

    #[cfg(all(unix, not(target_os = "macos")))]
    {
      let mut confirmation = false;
      handle.wait_for_action(|s| {
        confirmation = s == "accept";
      });
      Ok(confirmation)
    }

    #[cfg(target_os = "macos")]
    Ok(false)
  }
}

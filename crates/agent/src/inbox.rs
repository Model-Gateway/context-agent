//! Steering: messages sent to a running turn. The runtime opens its inbox
//! when a run starts, hands what arrived to the model before each step
//! (as user messages, reported with `AgentEvent::Steered`) and closes it
//! when the run ends. A run that is about to finish while messages wait
//! keeps going instead, so nothing sent before the close is dropped; a
//! message sent after it is refused and the sender runs it as a new turn.

use ctx_core::UserMessage;
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct Inbox(Arc<Mutex<InboxState>>);

#[derive(Default)]
struct InboxState {
    open: bool,
    waiting: Vec<UserMessage>,
}

impl Inbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hands `message` to the running turn; gives it back when no run is
    /// accepting messages (none started, or the run is ending).
    pub fn send(&self, message: UserMessage) -> Result<(), UserMessage> {
        let mut state = self.0.lock();
        if !state.open {
            return Err(message);
        }
        state.waiting.push(message);
        Ok(())
    }

    /// Messages sent but not yet handed to the model (after the run: the
    /// ones a cancelled or failed run never delivered).
    pub fn take(&self) -> Vec<UserMessage> {
        std::mem::take(&mut self.0.lock().waiting)
    }

    /// Starts accepting messages; the runtime does this when a run starts,
    /// an owner that queues the run first may do it earlier.
    pub fn open(&self) {
        self.0.lock().open = true;
    }

    /// Closes the inbox unless messages wait; `false` means the run has
    /// more to read and must not finish yet.
    pub(crate) fn close_if_empty(&self) -> bool {
        let mut state = self.0.lock();
        if !state.waiting.is_empty() {
            return false;
        }
        state.open = false;
        true
    }

    /// Closes the inbox; what is still waiting stays for [`Self::take`].
    pub(crate) fn close(&self) {
        self.0.lock().open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(text: &str) -> UserMessage {
        UserMessage { text: text.into(), ..Default::default() }
    }

    #[test]
    fn refuses_until_opened_and_after_close() {
        let inbox = Inbox::new();
        assert!(inbox.send(msg("early")).is_err());
        inbox.open();
        assert!(inbox.send(msg("one")).is_ok());
        assert!(!inbox.close_if_empty());
        assert_eq!(inbox.take().len(), 1);
        assert!(inbox.close_if_empty());
        assert!(inbox.send(msg("late")).is_err());
    }
}

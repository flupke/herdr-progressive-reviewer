use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use herdr_client::protocol::{Agent, AgentPort, AgentTarget, HerdrEvent, PaneId};
use review_mcp::{Operation, Response};
use review_store::ReviewStore;
use review_threads::{Post, Resolution, ReviewThreads, SavedDrafts, ThreadCommand, WakeupFailure};
use review_types::ReviewUnit;

use crate::delivery::{Courier, PromptError, PromptGate, PromptQueue};
use crate::{Command, Event, Input, access::Access, notification::Notification, wakeup::Wakeup};

pub(super) struct State {
    store: ReviewStore,
    port: Arc<dyn AgentPort>,
    target: AgentTarget,
    available: bool,
    books: HashMap<ReviewUnit, ReviewThreads>,
    access: HashMap<String, Access>,
    notifications: HashMap<ReviewUnit, Notification>,
    wakeups: HashMap<String, Wakeup>,
    prompts: PromptQueue,
    courier: Courier,
    /// Where the outcomes of comment notifications come back, behind the other inputs.
    inputs: Sender<Input>,
    publish: Box<dyn Fn(Event) + Send>,
}

impl State {
    pub(super) fn new(
        store: ReviewStore,
        port: Arc<dyn AgentPort>,
        target: AgentTarget,
        available: bool,
        publish: Box<dyn Fn(Event) + Send>,
        inputs: Sender<Input>,
        gate: Arc<PromptGate>,
    ) -> Self {
        Self {
            store,
            courier: Courier::start(port.clone(), gate),
            inputs,
            port,
            target,
            available,
            books: HashMap::new(),
            access: HashMap::new(),
            notifications: HashMap::new(),
            wakeups: HashMap::new(),
            prompts: PromptQueue::default(),
            publish,
        }
    }

    pub(super) fn run(mut self, receiver: &Receiver<Input>) {
        loop {
            let input = if self.has_pending_notifications() {
                receiver.recv_timeout(Duration::from_millis(100))
            } else {
                receiver.recv().map_err(|_| RecvTimeoutError::Disconnected)
            };
            match input {
                Ok(input) => {
                    if self.input(input).is_break() {
                        return;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.poll();
        }
    }

    /// Handle one input; break once the owner stops the worker.
    fn input(&mut self, input: Input) -> ControlFlow<()> {
        match input {
            Input::Ui(command) => self.command(command),
            Input::Prompt(request) => self.prompts.push(request),
            #[cfg(any(test, feature = "flush"))]
            Input::Flush(done) => {
                let _ = done.send(());
            }
            Input::Mcp(request) => {
                let result = self.request(&request.access, &request.operation);
                request.respond(result);
            }
            Input::Notified {
                token,
                through,
                result,
            } => self.notified(&token, through, result),
            Input::Stop => return ControlFlow::Break(()),
        }
        ControlFlow::Continue(())
    }

    /// The notification sent for access `token`, for the comments `through` a sequence, has
    /// its outcome. One that failed is not sent again until the reviewer retries or posts
    /// another comment, which a wakeup requested since then already did.
    fn notified(&mut self, token: &str, through: u64, result: Result<(), PromptError>) {
        let Err(error) = result else {
            return;
        };
        if self
            .wakeups
            .get(token)
            .is_some_and(|wakeup| wakeup.unchanged_since(through))
        {
            self.wakeups.remove(token);
        }
        let unit = self
            .access
            .get(token)
            .map(|access| access.review_unit.clone());
        self.report_wakeup(
            unit.as_ref(),
            Some(WakeupFailure {
                through,
                error: error.to_string(),
            }),
        );
    }

    /// The wakeup for the pending comments of `unit` cannot reach the agent: say why, and
    /// which comments wait.
    fn undelivered(&self, unit: Option<&ReviewUnit>, error: String) {
        match unit
            .and_then(|unit| self.books.get(unit))
            .and_then(ReviewThreads::pending_comment_sequence)
        {
            Some(through) => self.report_wakeup(unit, Some(WakeupFailure { through, error })),
            None => (self.publish)(Event::Error(error)),
        }
    }

    /// Publish what became of the latest wakeup for `unit`: `None` while one is on its way, or
    /// why one did not reach the agent. Without a review or a waiting comment to name, a
    /// failure is only an error.
    fn report_wakeup(&self, unit: Option<&ReviewUnit>, failure: Option<WakeupFailure>) {
        match unit {
            Some(unit) => (self.publish)(Event::Wakeup {
                review_unit: unit.clone(),
                failure,
            }),
            None => {
                if let Some(failure) = failure {
                    (self.publish)(Event::Error(failure.error));
                }
            }
        }
    }

    fn has_pending_notifications(&self) -> bool {
        self.prompts.is_pending()
            || !self.notifications.is_empty()
            || self.wakeups.iter().any(|(token, wakeup)| {
                wakeup.needs_poll()
                    && self.access.get(token).is_some_and(|access| {
                        self.books
                            .get(&access.review_unit)
                            .is_some_and(ReviewThreads::has_new_messages)
                    })
            })
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Thread(command) => self.thread_command(command),
            Command::ActiveAgentChanged => self.refresh_target(),
            Command::Observe(event) => self.observe(event),
        }
    }

    fn thread_command(&mut self, command: ThreadCommand) {
        match command {
            ThreadCommand::Load(unit) => self.load_review(unit),
            ThreadCommand::SaveDraft { review_unit, draft } => {
                let result =
                    self.update_drafts(&review_unit, |drafts, threads| drafts.save(draft, threads));
                self.report(result);
            }
            ThreadCommand::DiscardDraft {
                review_unit,
                thread_id,
            } => {
                let result = self.update_drafts(&review_unit, |drafts, _| {
                    drafts.discard(&thread_id);
                    Ok(())
                });
                self.report(result);
            }
            ThreadCommand::Post { review_unit, post } => self.post_comment(&review_unit, post),
            ThreadCommand::SetResolution {
                review_unit,
                thread_id,
                resolution,
            } => {
                let result = self.set_resolution(&review_unit, &thread_id, resolution);
                self.report(result);
            }
            ThreadCommand::MarkRepliesRead {
                review_unit,
                messages,
            } => {
                let result = self.update(&review_unit, |book| {
                    book.mark_replies_read(&messages);
                    Ok(())
                });
                self.report(result);
            }
            ThreadCommand::MarkRead {
                review_unit,
                thread_id,
                through,
            } => {
                let result = self.update(&review_unit, |book| book.mark_read(&thread_id, through));
                self.report(result);
            }
            ThreadCommand::Retry {
                review_unit,
                thread_id,
            } => {
                let result = self
                    .load(&review_unit)
                    .and_then(|book| book.retry(&thread_id))
                    .and_then(|()| self.schedule(&review_unit, true));
                self.report(result);
            }
        }
    }

    fn load_review(&mut self, unit: ReviewUnit) {
        let mut result = self.load(&unit);
        let mut drafts = SavedDrafts::default();
        if result.is_ok() {
            drafts = self.drafts_to_restore(&unit);
            self.resume(&unit);
            result = Ok(self.books[&unit].clone());
        }
        (self.publish)(Event::Loaded(ui_events::ReviewThreadsLoaded {
            review_unit: unit,
            result,
            drafts,
        }));
    }

    /// The latest drafts saved for `unit`, as every reviewer process sees them.
    /// Unreadable drafts are reported but never hide the threads; they stay on disk.
    fn drafts_to_restore(&self, unit: &ReviewUnit) -> SavedDrafts {
        self.store.load_drafts(unit).unwrap_or_else(|error| {
            (self.publish)(Event::Error(format!("Could not load drafts: {error}")));
            SavedDrafts::default()
        })
    }

    fn set_resolution(
        &mut self,
        unit: &ReviewUnit,
        id: &review_threads::ThreadId,
        resolution: Resolution,
    ) -> Result<(), String> {
        self.update(unit, |book| book.set_resolution(id, resolution))?;
        if resolution == Resolution::Open {
            self.schedule(unit, true)?;
        } else if !self.books[unit].has_new_messages() {
            self.cancel_wakeups(unit);
        }
        Ok(())
    }

    fn post_comment(&mut self, review_unit: &ReviewUnit, post: Post) {
        let message_id = post.message().id.clone();
        let result = self.update(review_unit, |book| book.post(post).map(|_| ()));
        let posted = result.is_ok();
        if posted {
            // Updating the drafts forgets every draft whose post the threads now hold. A
            // crash before this write cannot restore the draft: loading forgets it too.
            let forgotten = self.update_drafts(review_unit, |_, _| Ok(()));
            self.report(forgotten);
        }
        (self.publish)(Event::Posted(ui_events::ThreadPostFinished {
            review_unit: review_unit.clone(),
            message_id,
            result,
        }));
        if posted && let Err(error) = self.schedule(review_unit, false) {
            self.undelivered(Some(review_unit), error);
        }
    }

    fn load(&mut self, unit: &ReviewUnit) -> Result<ReviewThreads, String> {
        let book = self
            .store
            .load_threads(unit)
            .map_err(|error| error.to_string())?;
        self.books.insert(unit.clone(), book.clone());
        Ok(book)
    }

    fn update<T>(
        &mut self,
        unit: &ReviewUnit,
        update: impl FnOnce(&mut ReviewThreads) -> Result<T, String>,
    ) -> Result<T, String> {
        let (result, book) = self
            .store
            .update_threads(unit, update)
            .map_err(|error| error.to_string())?;
        let previous = self.books.insert(book.review_unit.clone(), book.clone());
        if previous.as_ref() != Some(&book) {
            let drafts = self.drafts_to_restore(&book.review_unit);
            (self.publish)(Event::Loaded(ui_events::ReviewThreadsLoaded {
                review_unit: book.review_unit.clone(),
                result: Ok(book),
                drafts,
            }));
        }
        Ok(result)
    }

    /// Change the saved drafts. The posted threads are never rewritten for a draft.
    fn update_drafts(
        &mut self,
        unit: &ReviewUnit,
        update: impl FnOnce(&mut SavedDrafts, &ReviewThreads) -> Result<(), String>,
    ) -> Result<(), String> {
        self.store
            .update_drafts(unit, update)
            .map(|((), _)| ())
            .map_err(|error| error.to_string())
    }

    fn refresh_target(&mut self) {
        for unit in self.books.keys().cloned().collect::<Vec<_>>() {
            self.resume(&unit);
        }
    }

    fn cancel_wakeups(&mut self, unit: &ReviewUnit) {
        self.notifications.remove(unit);
        self.wakeups.retain(|token, _| {
            self.access
                .get(token)
                .is_some_and(|access| &access.review_unit != unit)
        });
    }

    fn schedule(&mut self, unit: &ReviewUnit, retry: bool) -> Result<(), String> {
        let book = self
            .books
            .get(unit)
            .ok_or("The review history is not loaded")?;
        if !book.has_new_messages() {
            return Ok(());
        }
        if !self.available {
            return Err("The comment is saved, but the reviewer MCP server is unavailable".into());
        }
        let notification = self.notifications.entry(unit.clone()).or_default();
        notification.retry |= retry;
        Ok(())
    }

    fn grant(&mut self, unit: &ReviewUnit, agent: Agent) -> Result<Access, String> {
        for access in self.access.values() {
            if access.review_unit == *unit && access.matches_agent(&*self.port, &agent)? {
                return Ok(access.clone());
            }
        }
        let access = Access::new(unit.clone(), agent, &*self.port)?;
        self.cancel_wakeups(unit);
        self.access
            .retain(|_, previous| previous.review_unit != *unit);
        self.access.insert(access.token.clone(), access.clone());
        Ok(access)
    }

    fn resume(&mut self, unit: &ReviewUnit) {
        if self
            .books
            .get(unit)
            .is_some_and(ReviewThreads::has_new_messages)
        {
            let result = self.schedule(unit, false);
            self.report(result);
        }
    }

    fn request_wakeup(&mut self, access: &Access, retry: bool) {
        let Some(sequence) = self
            .books
            .get(&access.review_unit)
            .and_then(ReviewThreads::pending_comment_sequence)
        else {
            return;
        };
        self.wakeups
            .entry(access.token.clone())
            .or_default()
            .request(sequence, retry);
    }

    fn request(&mut self, access: &str, operation: &Operation) -> Result<Response, String> {
        let access =
            self.access.get(access).cloned().ok_or(
                "Unknown review access value; use the value in the latest reviewer wakeup",
            )?;
        access.current_agent(&*self.port)?;
        let book = self.load(&access.review_unit)?;
        match operation {
            Operation::SubmitQuestion(_) | Operation::SubmitConclusion(_) => {
                Err("Explore requests belong to the interview owner".into())
            }
            Operation::ListThreads => Ok(Response::Threads(book.threads().to_vec())),
            Operation::GetThread(id) => {
                let thread = book
                    .thread(id)
                    .cloned()
                    .ok_or("The review thread no longer exists")?;
                Ok(Response::Threads(vec![thread]))
            }
            Operation::GetNewMessages => {
                let threads = book.new_messages();
                Ok(Response::Threads(threads))
            }
            Operation::Reply(post) => {
                let id = self.update(&access.review_unit, |book| book.answer(post.clone()))?;
                if !self.books[&access.review_unit].has_new_messages()
                    && let Some(wakeup) = self.wakeups.get_mut(&access.token)
                {
                    wakeup.answered();
                }
                Ok(Response::Posted(id))
            }
        }
    }

    fn observe(&mut self, event: HerdrEvent) {
        match event {
            HerdrEvent::AgentDetected {
                pane_id,
                released: true,
                ..
            } => {
                // An agent can resume in the same pane after MCP setup.
                // Keep its grant; every request still checks the live identity.
                self.interrupt_wakeups(&pane_id);
                self.refresh_target();
            }
            HerdrEvent::AgentDetected {
                pane_id,
                released: false,
                ..
            } => {
                self.resume_access(&pane_id);
                self.refresh_target();
            }
            _ => {}
        }
    }

    fn resume_access(&mut self, pane: &PaneId) {
        let valid = self
            .access
            .values()
            .filter(|access| {
                &access.agent.pane_id == pane
                    && !self.wakeups.contains_key(&access.token)
                    && access.current_agent(&*self.port).is_ok()
            })
            .cloned()
            .collect::<Vec<_>>();
        for access in valid {
            self.request_wakeup(&access, false);
        }
    }

    fn interrupt_wakeups(&mut self, pane: &PaneId) {
        for (token, access) in &self.access {
            if access.agent.pane_id == *pane
                && let Some(wakeup) = self.wakeups.get_mut(token)
            {
                wakeup.interrupted();
            }
        }
    }

    fn poll(&mut self) {
        self.prompts.poll(&*self.port, &self.courier);
        for unit in self.notifications.keys().cloned().collect::<Vec<_>>() {
            if let Err(error) = self.prepare_notification(&unit) {
                self.notifications.remove(&unit);
                self.undelivered(Some(&unit), error);
            }
        }
        let tokens = self
            .wakeups
            .iter()
            .filter(|(_, wakeup)| wakeup.needs_poll())
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        for token in tokens {
            if let Err(error) = self.notify(&token) {
                self.wakeups.remove(&token);
                let unit = self
                    .access
                    .get(&token)
                    .map(|access| access.review_unit.clone());
                self.undelivered(unit.as_ref(), error);
            }
        }
    }

    fn prepare_notification(&mut self, unit: &ReviewUnit) -> Result<(), String> {
        let retry = self
            .notifications
            .get(unit)
            .expect("pending notification")
            .retry;
        if !self
            .books
            .get(unit)
            .is_some_and(ReviewThreads::has_new_messages)
        {
            self.cancel_wakeups(unit);
            return Ok(());
        }
        let agent = self
            .target
            .resolve(&*self.port)
            .map_err(|error| error.to_string())?
            .ok_or("Focus an implementation agent to receive the saved comments")?;
        let access = self.grant(unit, agent)?;
        self.notifications.remove(unit);
        self.request_wakeup(&access, retry);
        Ok(())
    }

    fn notify(&mut self, token: &str) -> Result<(), String> {
        let Some(access) = self.access.get(token) else {
            return Ok(());
        };
        let pending = self
            .books
            .get(&access.review_unit)
            .map(ReviewThreads::new_messages)
            .unwrap_or_default();
        if pending.is_empty() {
            return Ok(());
        }
        let current = self
            .target
            .resolve(&*self.port)
            .map_err(|error| error.to_string())?
            .ok_or("Focus an implementation agent to receive the saved comments")?;
        if !access.matches_agent(&*self.port, &current)? {
            let unit = access.review_unit.clone();
            self.wakeups.remove(token);
            return self.schedule(&unit, false);
        }
        let prompt = access.prompt(&pending);
        let Some(wakeup) = self.wakeups.get_mut(token) else {
            return Ok(());
        };
        if wakeup.needs_poll() {
            let through = wakeup.sent();
            let unit = access.review_unit.clone();
            self.report_wakeup(Some(&unit), None);
            let inputs = self.inputs.clone();
            let token = token.to_owned();
            self.courier.send(current, prompt, None, move |result| {
                let _ = inputs.send(Input::Notified {
                    token,
                    through,
                    result,
                });
            });
        }
        Ok(())
    }

    fn report(&self, result: Result<(), String>) {
        if let Err(error) = result {
            (self.publish)(Event::Error(error));
        }
    }
}

#[cfg(test)]
#[path = "state.tests.rs"]
mod tests;

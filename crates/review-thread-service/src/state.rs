use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, AgentPrompter, AgentTarget, HerdrEvent, PaneId};
use review_mcp::{Operation, Response};
use review_store::ReviewStore;
use review_threads::{Post, Resolution, ReviewThreads, ThreadCommand};
use review_types::ReviewUnit;

use crate::{Command, Event, Input, access::Access, notification::Notification, wakeup::Wakeup};

pub(super) struct State {
    store: ReviewStore,
    client: HerdrClient,
    target: AgentTarget,
    available: bool,
    books: HashMap<ReviewUnit, ReviewThreads>,
    access: HashMap<String, Access>,
    notifications: HashMap<ReviewUnit, Notification>,
    wakeups: HashMap<String, Wakeup>,
    prompts: crate::delivery::PromptQueue,
    publish: Box<dyn Fn(Event) + Send>,
}

impl State {
    pub(super) fn new(
        store: ReviewStore,
        client: HerdrClient,
        target: AgentTarget,
        available: bool,
        publish: Box<dyn Fn(Event) + Send>,
    ) -> Self {
        Self {
            store,
            client,
            target,
            available,
            books: HashMap::new(),
            access: HashMap::new(),
            notifications: HashMap::new(),
            wakeups: HashMap::new(),
            prompts: crate::delivery::PromptQueue::default(),
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
                Ok(Input::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Input::Ui(command)) => self.command(command),
                Ok(Input::Prompt(request)) => self.prompts.push(request),
                Ok(Input::Mcp(request)) => {
                    self.dispatch_mcp(request);
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.poll();
        }
    }

    fn dispatch_mcp(&mut self, request: review_mcp::Request) {
        if matches!(
            request.operation,
            Operation::SubmitQuestion(_) | Operation::SubmitConclusion(_)
        ) {
            (self.publish)(Event::Explore(request));
        } else {
            let result = self.request(&request);
            request.respond(result);
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
                let result = self.update_draft(&review_unit, |book| book.save_draft(draft));
                self.report(result);
            }
            ThreadCommand::DiscardDraft {
                review_unit,
                target,
            } => {
                let result = self.update_draft(&review_unit, |book| {
                    book.discard_draft(&target);
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
        if result.is_ok() {
            self.resume(&unit);
            result = Ok(self.books[&unit].clone());
        }
        (self.publish)(Event::Loaded(ui_events::ReviewThreadsLoaded {
            review_unit: unit,
            result,
        }));
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
        (self.publish)(Event::Posted(ui_events::ThreadPostFinished {
            review_unit: review_unit.clone(),
            message_id,
            result,
        }));
        if posted {
            let notify = self.schedule(review_unit, false);
            self.report(notify);
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
            (self.publish)(Event::Loaded(ui_events::ReviewThreadsLoaded {
                review_unit: book.review_unit.clone(),
                result: Ok(book),
            }));
        }
        Ok(result)
    }

    fn update_draft(
        &mut self,
        unit: &ReviewUnit,
        update: impl FnOnce(&mut ReviewThreads) -> Result<(), String>,
    ) -> Result<(), String> {
        let ((), book) = self
            .store
            .update_threads(unit, update)
            .map_err(|error| error.to_string())?;
        self.books.insert(unit.clone(), book);
        Ok(())
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
            if access.review_unit == *unit && access.matches_agent(&self.client, &agent)? {
                return Ok(access.clone());
            }
        }
        let access = Access::new(unit.clone(), agent, &self.client)?;
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

    fn request(&mut self, request: &review_mcp::Request) -> Result<Response, String> {
        let access =
            self.access.get(&request.access).cloned().ok_or(
                "Unknown review access value; use the value in the latest reviewer wakeup",
            )?;
        access.current_agent(&self.client)?;
        let book = self.load(&access.review_unit)?;
        match &request.operation {
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
                    && access.current_agent(&self.client).is_ok()
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
        self.prompts.poll(&self.client);
        for unit in self.notifications.keys().cloned().collect::<Vec<_>>() {
            if let Err(error) = self.prepare_notification(&unit) {
                self.notifications.remove(&unit);
                (self.publish)(Event::Error(error));
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
                (self.publish)(Event::Error(error));
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
            .resolve(&self.client)
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
        let unread = self
            .books
            .get(&access.review_unit)
            .is_some_and(ReviewThreads::has_new_messages);
        if !unread {
            return Ok(());
        }
        let current = self
            .target
            .resolve(&self.client)
            .map_err(|error| error.to_string())?
            .ok_or("Focus an implementation agent to receive the saved comments")?;
        if !access.matches_agent(&self.client, &current)? {
            let unit = access.review_unit.clone();
            self.wakeups.remove(token);
            return self.schedule(&unit, false);
        }
        let Some(wakeup) = self.wakeups.get_mut(token) else {
            return Ok(());
        };
        if wakeup.needs_poll() {
            wakeup.sent();
            self.client
                .prompt_agent(&current.pane_id, &access.prompt())
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn report(&self, result: Result<(), String>) {
        if let Err(error) = result {
            (self.publish)(Event::Error(error));
        }
    }
}

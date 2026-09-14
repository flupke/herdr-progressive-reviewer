use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use herdr_client::client::HerdrClient;
use herdr_client::protocol::{Agent, AgentPrompter, AgentTarget, HerdrEvent, PaneId};
use review_mcp::{Operation, Response};
use review_store::ReviewStore;
use review_threads::{Post, ReviewThread, ReviewThreads, ThreadCommand, ThreadId};
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
                Ok(Input::Mcp(request)) => {
                    let result = self.request(&request);
                    request.respond(result);
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.poll();
        }
    }

    fn has_pending_notifications(&self) -> bool {
        !self.notifications.is_empty()
            || self.wakeups.keys().any(|token| {
                self.access.get(token).is_some_and(|access| {
                    self.books
                        .get(&access.review_unit)
                        .is_some_and(|book| book.has_new_messages(&access.reader))
                })
            })
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Thread(command) => self.thread_command(command),
            Command::Focus(pane) => self.target.observe_focus(&pane),
            Command::Observe(event) => self.observe(event),
        }
    }

    fn thread_command(&mut self, command: ThreadCommand) {
        match command {
            ThreadCommand::Load(unit) => {
                let result = self.load(&unit);
                if result.is_ok() {
                    self.resume(&unit);
                }
                (self.publish)(Event::Loaded(ui_events::ReviewThreadsLoaded {
                    review_unit: unit,
                    result,
                }));
            }
            ThreadCommand::Post { review_unit, post } => self.post_comment(&review_unit, post),
            ThreadCommand::SetResolution {
                review_unit,
                thread_id,
                resolution,
            } => {
                let result = self.update(&review_unit, |book| {
                    book.set_resolution(&thread_id, resolution)
                });
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
                let result = self.schedule(&review_unit, Some(thread_id));
                self.report(result);
            }
        }
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
            let notify = self.schedule(review_unit, None);
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

    fn schedule(&mut self, unit: &ReviewUnit, retry: Option<ThreadId>) -> Result<(), String> {
        if !self.available {
            return Err("The comment is saved, but the reviewer MCP server is unavailable".into());
        }
        let agent = self
            .target
            .resolve(&self.client)
            .map_err(|error| error.to_string())?
            .ok_or("Focus the agent that should receive this review, then post a follow-up in the reviewer")?;
        let notification = self
            .notifications
            .entry(unit.clone())
            .or_insert_with(|| Notification::new(agent.clone()));
        notification.agent = agent;
        notification.retries.extend(retry);
        Ok(())
    }

    fn grant(&mut self, unit: &ReviewUnit, agent: Agent) -> Result<Access, String> {
        if let Some(access) = self
            .access
            .values()
            .find(|access| access.review_unit == *unit && access.matches_agent(&agent))
        {
            return Ok(access.clone());
        }
        let access = Access::new(unit.clone(), agent)?;
        self.access.insert(access.token.clone(), access.clone());
        Ok(access)
    }

    fn resume(&mut self, unit: &ReviewUnit) {
        if self
            .books
            .get(unit)
            .is_some_and(|book| !book.threads().is_empty())
        {
            let result = self.schedule(unit, None);
            self.report(result);
        }
    }

    fn request_wakeup(&mut self, access: &Access, retry: bool) {
        let Some(book) = self.books.get(&access.review_unit) else {
            return;
        };
        self.wakeups
            .entry(access.token.clone())
            .or_default()
            .request(book.sequence(), retry);
    }

    fn request(&mut self, request: &review_mcp::Request) -> Result<Response, String> {
        let access =
            self.access.get(&request.access).cloned().ok_or(
                "Unknown review access value; use the value in the latest reviewer wakeup",
            )?;
        access.current_agent(&self.client)?;
        let book = self.load(&access.review_unit)?;
        match &request.operation {
            Operation::ListThreads => Ok(Response::Threads(book.threads().to_vec())),
            Operation::GetThread(id) => {
                let thread = book
                    .thread(id)
                    .cloned()
                    .ok_or("The review thread no longer exists")?;
                self.retrieved(&access, book.sequence(), std::slice::from_ref(&thread))?;
                Ok(Response::Threads(vec![thread]))
            }
            Operation::GetNewMessages => {
                let threads = book.new_messages(&access.reader);
                self.retrieved(&access, book.sequence(), &threads)?;
                Ok(Response::Threads(threads))
            }
            Operation::Reply {
                thread_id,
                message_id,
                text,
            } => {
                let id = self.update(&access.review_unit, |book| {
                    book.post(Post::agent_reply(
                        thread_id.clone(),
                        message_id.clone(),
                        text.clone(),
                    ))
                })?;
                Ok(Response::Posted(id))
            }
        }
    }

    fn retrieved(
        &mut self,
        access: &Access,
        through: u64,
        threads: &[ReviewThread],
    ) -> Result<(), String> {
        let all_retrieved = self.update(&access.review_unit, |book| {
            for thread in threads {
                book.mark_retrieved(&access.reader, &thread.id, through);
            }
            Ok(!book.has_new_messages(&access.reader))
        })?;
        if all_retrieved && let Some(wakeup) = self.wakeups.get_mut(&access.token) {
            wakeup.retrieved();
        }
        Ok(())
    }

    fn observe(&mut self, event: HerdrEvent) {
        match event {
            HerdrEvent::AgentStatusChanged {
                pane_id, status, ..
            } => {
                self.observe_wakeups(&pane_id, |wakeup| wakeup.observe(status));
            }
            HerdrEvent::AgentDetected {
                pane_id,
                released: true,
                ..
            } => {
                // A native session can resume in the same pane after MCP setup.
                // Keep its grant; every request still checks the live identity.
                self.observe_wakeups(&pane_id, Wakeup::interrupted);
            }
            HerdrEvent::AgentDetected {
                released: false, ..
            } => {
                for unit in self.books.keys().cloned().collect::<Vec<_>>() {
                    if !self.notifications.contains_key(&unit)
                        && !self
                            .access
                            .values()
                            .any(|access| access.review_unit == unit)
                    {
                        self.resume(&unit);
                    }
                }
            }
            _ => {}
        }
    }

    fn observe_wakeups(&mut self, pane: &PaneId, observe: impl Fn(&mut Wakeup)) {
        for (token, access) in &self.access {
            if access.agent.pane_id == *pane
                && let Some(wakeup) = self.wakeups.get_mut(token)
            {
                observe(wakeup);
            }
        }
    }

    fn poll(&mut self) {
        for unit in self.notifications.keys().cloned().collect::<Vec<_>>() {
            if let Err(error) = self.prepare_notification(&unit) {
                self.notifications.remove(&unit);
                (self.publish)(Event::Error(error));
            }
        }
        let tokens = self.wakeups.keys().cloned().collect::<Vec<_>>();
        for token in tokens {
            if let Err(error) = self.notify(&token) {
                self.wakeups.remove(&token);
                (self.publish)(Event::Error(error));
            }
        }
    }

    fn prepare_notification(&mut self, unit: &ReviewUnit) -> Result<(), String> {
        let Some(notification) = self.notifications.get(unit) else {
            return Ok(());
        };
        let Some(agent) = notification.current_agent(&self.client)? else {
            return Ok(());
        };
        let access = self.grant(unit, agent)?;
        let notification = self
            .notifications
            .remove(unit)
            .expect("pending notification");
        let retry = !notification.retries.is_empty();
        for thread in notification.retries {
            self.update(unit, |book| book.retry(&access.reader, &thread))?;
        }
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
            .is_some_and(|book| book.has_new_messages(&access.reader));
        if !unread {
            return Ok(());
        }
        // Wait through a restart without losing unread work or repeatedly
        // reporting the absent agent. A different session cannot use this grant.
        let Ok(current) = access.current_agent(&self.client) else {
            return Ok(());
        };
        let Some(wakeup) = self.wakeups.get_mut(token) else {
            return Ok(());
        };
        wakeup.observe(current.agent_status);
        if wakeup.should_notify(current.agent_status, unread) {
            if !crate::prompt::PromptGate::ready(&self.client, &current) {
                if wakeup.defer() {
                    (self.publish)(Event::NotificationDeferred);
                }
                return Ok(());
            }
            let result = self
                .client
                .prompt_agent(&current.pane_id, &access.prompt())
                .map_err(|error| error.to_string());
            wakeup.sent(result.is_ok());
            result?;
        }
        Ok(())
    }

    fn report(&self, result: Result<(), String>) {
        if let Err(error) = result {
            (self.publish)(Event::Error(error));
        }
    }
}

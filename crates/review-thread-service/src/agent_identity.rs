//! Decides whether a live agent is still the agent session a grant or prompt was bound to.
//!
//! Review access and the pinned agent share this decision. Their remaining
//! differences are the named [`IdentityRules`] and the follow mode of a
//! [`SessionBinding`]; each caller maps a [`Verdict`] to its own errors.

use herdr_client::protocol::{Agent, AgentPort, AgentSession, PaneId, WorkspaceId};

/// How a native session reported now is compared with the bound one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionComparison {
    /// Every field, including the reporting `source`, must be equal.
    Exact,
    /// The agent, kind and value must be equal; the reporting `source` may differ.
    IgnoringSource,
}

impl SessionComparison {
    fn same(self, bound: &AgentSession, current: &AgentSession) -> bool {
        match self {
            Self::Exact => bound == current,
            Self::IgnoringSource => {
                bound.agent == current.agent
                    && bound.kind == current.kind
                    && bound.value == current.value
            }
        }
    }
}

/// What happens to a foreground process group binding once a native session appears.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProcessBinding {
    /// The process group stays part of the identity for its whole life.
    KeptAfterSession,
    /// The first observed native session replaces the process group check.
    ReleasedBySession,
}

/// The rules that differ between the callers of [`AgentIdentity`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IdentityRules {
    session: SessionComparison,
    process: ProcessBinding,
}

impl IdentityRules {
    /// Review access values are bearer tokens: every recorded identity stays checked.
    pub(crate) const REVIEW_ACCESS: Self = Self {
        session: SessionComparison::Exact,
        process: ProcessBinding::KeptAfterSession,
    };
    /// A pinned prompt recipient may resume its native session in a new process.
    pub(crate) const PINNED_AGENT: Self = Self {
        session: SessionComparison::IgnoringSource,
        process: ProcessBinding::ReleasedBySession,
    };
}

/// The native session an identity accepts.
#[derive(Clone, Debug, Eq, PartialEq)]
enum SessionBinding {
    /// Accept any session and remember the latest one until [`AgentIdentity::seal`].
    Following(Option<AgentSession>),
    /// No session is known yet; the first one observed becomes bound.
    Unbound,
    Bound(AgentSession),
}

/// Why a live agent is not the same agent session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Change {
    /// The pane, workspace or agent implementation differs.
    Agent,
    /// The pane's foreground process group differs.
    ProcessGroup,
    /// The pane reports a different native session.
    Session,
}

/// The outcome of comparing a live agent with an identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Verdict {
    Same,
    /// A session is bound but the pane reports none right now.
    SessionMissing,
    Changed(Change),
}

/// Herdr could not answer an identity question.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum IdentityError {
    /// The port request itself failed.
    Port(String),
    /// Herdr reported no usable foreground process group for the pane.
    ProcessUnidentified,
}

impl IdentityError {
    /// The caller's message, naming what an unidentified process means to it.
    pub(crate) fn into_message(self, process_unidentified: &str) -> String {
        match self {
            Self::Port(error) => error,
            Self::ProcessUnidentified => process_unidentified.to_owned(),
        }
    }
}

/// The agent session one grant or prompt is bound to.
#[derive(Clone, Debug)]
pub(crate) struct AgentIdentity {
    pane_id: PaneId,
    workspace_id: WorkspaceId,
    agent: Option<String>,
    session: SessionBinding,
    process_group: Option<u32>,
    rules: IdentityRules,
}

impl AgentIdentity {
    /// Bind to `agent`'s reported native session, or to none when it reports none.
    pub(crate) fn new(agent: &Agent, rules: IdentityRules) -> Self {
        Self {
            pane_id: agent.pane_id.clone(),
            workspace_id: agent.workspace_id.clone(),
            agent: agent.agent.clone(),
            session: agent
                .agent_session
                .clone()
                .map_or(SessionBinding::Unbound, SessionBinding::Bound),
            process_group: None,
            rules,
        }
    }

    /// Accept any native session until [`Self::seal`].
    pub(crate) fn following(mut self) -> Self {
        self.session = SessionBinding::Following(match self.session {
            SessionBinding::Bound(session) => Some(session),
            SessionBinding::Following(session) => session,
            SessionBinding::Unbound => None,
        });
        self
    }

    /// Without a native session, bind to the pane's current foreground process group.
    pub(crate) fn bind_process_without_session(
        mut self,
        port: &dyn AgentPort,
    ) -> Result<Self, IdentityError> {
        if matches!(
            self.session,
            SessionBinding::Unbound | SessionBinding::Following(None)
        ) {
            self.process_group = Some(process_group(port, &self.pane_id)?);
        }
        Ok(self)
    }

    /// Stop following: the latest observed session, if any, becomes bound.
    pub(crate) fn seal(&mut self) {
        if let SessionBinding::Following(session) = &mut self.session {
            self.session = session
                .take()
                .map_or(SessionBinding::Unbound, SessionBinding::Bound);
        }
    }

    /// Compare `current` with this identity, adopting what a match reveals.
    pub(crate) fn check(
        &mut self,
        port: &dyn AgentPort,
        current: &Agent,
    ) -> Result<Verdict, IdentityError> {
        if current.pane_id != self.pane_id
            || current.workspace_id != self.workspace_id
            || current.agent != self.agent
        {
            return Ok(Verdict::Changed(Change::Agent));
        }
        if let Some(group) = self.process_group
            && process_group(port, &current.pane_id)? != group
        {
            return Ok(Verdict::Changed(Change::ProcessGroup));
        }
        let verdict = self.compare_session(current.agent_session.as_ref());
        if verdict == Verdict::Same {
            self.adopt(current.agent_session.as_ref());
        }
        Ok(verdict)
    }

    fn compare_session(&self, current: Option<&AgentSession>) -> Verdict {
        match (&self.session, current) {
            (SessionBinding::Following(_) | SessionBinding::Unbound, _) => Verdict::Same,
            (SessionBinding::Bound(_), None) => Verdict::SessionMissing,
            (SessionBinding::Bound(bound), Some(current)) => {
                if self.rules.session.same(bound, current) {
                    Verdict::Same
                } else {
                    Verdict::Changed(Change::Session)
                }
            }
        }
    }

    fn adopt(&mut self, current: Option<&AgentSession>) {
        if let SessionBinding::Following(latest) = &mut self.session {
            *latest = current.cloned();
        }
        let Some(current) = current else {
            return;
        };
        if self.session == SessionBinding::Unbound {
            self.session = SessionBinding::Bound(current.clone());
        }
        if self.rules.process == ProcessBinding::ReleasedBySession {
            self.process_group = None;
        }
    }
}

fn process_group(port: &dyn AgentPort, pane: &PaneId) -> Result<u32, IdentityError> {
    port.pane_process_info(pane)
        .map_err(|error| IdentityError::Port(error.to_string()))?
        .foreground_process_group_id
        .filter(|group| *group != 0)
        .ok_or(IdentityError::ProcessUnidentified)
}

#[cfg(test)]
#[path = "agent_identity.tests.rs"]
mod tests;

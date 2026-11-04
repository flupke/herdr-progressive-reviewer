# Talk to herdr-core behind the host traits, beside Herdr

The user now runs herdr-app, whose core, herdr-core (`hc`), runs the terminals and agents that
Herdr ran. The reviewer reached Herdr through `herdr-client`'s `HerdrClient`, which the review
pane, run-ahead's hosts and the thread service held, and through Herdr's events. Every test
that drives an agent runs on a private Herdr server, and herdr-core lacks some of what the
reviewer gets from Herdr: focus, the agent's session, a plugin system.

The reviewer talks to herdr-core through a backend of its own, the `hc-client` crate, behind
the traits of `herdr-client`: `AgentPort` for the agents, and `AgentHost`, which adds the events
the reviewer follows, an agent's statuses, and the session report of ADR 0012. Everything that
held `HerdrClient` for its agents holds an `Arc<dyn AgentHost>`. The review pane picks
Herdr when `HERDR_SOCKET_PATH` is set, and herdr-core when only `HC_TERMINAL_SOCKET` is. Herdr
stays the backend under Herdr and in the tests until the tests run on a private herdr-core;
then Herdr goes. What herdr-core lacks comes in later changes: the pane's opening, focus, and
the agent's session.

Considered:
- Replacing Herdr with herdr-core in one change: the tests would have no host until they are
  ported, and the reviewer would stop working under Herdr meanwhile.
- The backend inside `herdr-client`: it would double a crate the repository keeps small, and
  mix two protocols in one.

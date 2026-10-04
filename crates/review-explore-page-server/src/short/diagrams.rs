//! The explanation of the second fixed question: a Mermaid diagram that draws, wider than a
//! phone's screen, and one that Mermaid cannot parse.

pub(super) const RATIONALE: &str = "\
The kept draft can live in the round's record or in the editor state.

```mermaid
flowchart LR
  editor[\"Editor\"] -->|\"each keystroke\"| state[\"Editor state\"]
  editor -->|\"each answer\"| record[\"Round record\"]
  record --> reopened[\"Reopened round\"]
  state --> reopened
```

The round's record would hold it as follows; this sketch does not parse, because a label with \
parentheses needs quotes.

```mermaid
flowchart LR
  draft --> record[saved (with the answers)]
```";

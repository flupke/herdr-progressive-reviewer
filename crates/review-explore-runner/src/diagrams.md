Add a diagram only where it shows what text shows badly: who calls whom and in which order (a
sequence), a structure (a flowchart or a class diagram), or a state before and after the change
(a state diagram). Leave it out when it would only repeat a sentence, and prefer a table for a
comparison. The pane shows a diagram as its source, and the page draws it at its natural size:
lay a flowchart out top to bottom (`flowchart TD`), keep labels short and use at most about 12
nodes. To avoid parse errors, quote every label that contains punctuation, as in
`A["src/lib.rs (old)"]`; never put `;` in a sequence message, where it ends the statement; and
write no HTML, `click`, `style`, `classDef` or `%%{init}%%`: the page applies its own light and
dark theme. A diagram that does not parse shows as its source.

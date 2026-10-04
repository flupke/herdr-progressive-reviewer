Add a diagram only where it shows what text shows badly: who calls whom and in which order (a
sequence), a structure (a flowchart or a class diagram), or a state before and after the change
(a state diagram). Leave it out when it would only repeat a sentence, and prefer a table for a
comparison. The pane shows a diagram as its source, and the page draws it in its own light and
dark theme, at its natural size when it fits. A flowchart may run left to right
(`flowchart LR`), as a map of components does: the page draws it top to bottom when it does not
fit. Keep labels short, use at most about 12 nodes, and draw nodes with rounded corners, as in
`queue("ReplyQueue")`, not as stadiums. Class the components the change adds `new` and the ones
it changes `changed`, as in `queue("ReplyQueue"):::new`: the page colours them and adds a legend.
Number the messages of a sequence diagram with `autonumber`, and give it at most five
participants, beyond which it no longer fits the page's column. To avoid parse errors, quote
every label that contains punctuation, as in `A["src/lib.rs (old)"]`; never put `;` in a
sequence message, where it ends the statement; and write no HTML, `click`, `style`, `classDef`
or `%%{init}%%`. A diagram that does not parse shows as its source.

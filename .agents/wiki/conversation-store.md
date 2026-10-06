# Conversation store

How review threads, their original code and drafts are saved, and what each store version migrates from.

State is separate for each canonical checkout. Conversation updates take a storage lock, so
posted messages survive while delivery callbacks and agent replies update their records.

## Version 4: the index and its context files

- Immutable original code is stored in compressed, content-addressed context files. The
  conversation index holds the messages and the read and answer progress, with references
  to that context.
- A small update rewrites the index without recompressing the original files, and open
  views share the cached context.
- Nothing is pruned, resolved history included. Former recipient fields are ignored when
  an existing index is read.

## Earlier versions

- Version 2 is read without dropping history. Its retrieval cursors alone never prove
  completion: only unresolved threads still waiting for an answer are delivered, and a
  final fetch does not make answered threads pending again.
- A legacy reply has no exact snapshot boundary, so it covers the comments before it in
  posting order, matching the thread's Waiting status; a follow-up after it stays pending.
  A reply with `in_reply_to` always uses its exact boundary, also when another comment
  arrived before the answer was saved.
- Version 3 acknowledgement positions are merged across recipients, which keeps completed
  answers during a handoff.
- The next changed update migrates version 2 or 3 to version 4 without dropping history.
- Older comment and queue files stay untouched.

## Drafts

- A draft is not part of the shared review thread. Drafts live in a separate compressed
  file for each review (`drafts/<review hash>.json.zst`, version 1), with its own lock and
  references to the same context files.
- Saving or discarding a draft never rewrites the conversation index.
- A post is written to the index first; the posted draft is discarded afterwards. Loading
  drafts drops any draft whose message ID is already in the index, so a posted comment
  never comes back as a draft, also after a crash between the two writes.
- Recovery opens a draft for editing. It never posts it or wakes an agent.
- Cancel, and submitting trimmed-empty text, discard the saved draft.
- An unreadable draft file is reported and never hides the threads.

## Drafts saved by earlier builds

Earlier builds kept drafts inside the conversation index. The first load moves them to the
draft file: it writes the draft file, then rewrites the index without them, still as
version 4. An interrupted move leaves the drafts in both files, and the next load repeats
it without duplicates. An earlier build still loads the migrated index and shows no drafts;
drafts it saves go back into the index and are moved again, replacing the draft file's copy
for the same thread. Drafts still inside the index stay there while the draft file is
unreadable.

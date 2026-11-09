# Plugin data handling

Plugins store data through the host, scoped to their declared tables and
interfaces. To-do, Notes, Habits and Bookmarks contain personal records; Voice
can retain audio/transcripts; Reports can send selected source material to the
host's configured AI provider. The host owns encryption, sync, secrets,
notifications and permitted service access.

A configured remote AI provider receives the selected content/instructions.
Clearing an active audio/text field does not prove deletion from CRDT history or
backups. Do not treat source filters as a prompt-injection security boundary.

Read the matching host's PRIVACY.md and SECURITY.md. This source preview does not
claim full at-rest encryption, complete retention/erasure or production approval.

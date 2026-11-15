# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Notes 0.5.3: Select moves into the view menu

- The idle "Select" link above the note list is gone. Select notes now sits in the "..." menu next to search, as in the Task manager; switching views ends a selection.
- An empty Projects group says "Give a note a project to see it here" instead of showing a bare title.
- Pure UI change: no storage, query or command changes.

### Notes 0.5.2: the editor reads like a note

- The note panel is no longer a form. It opens with a big, borderless title ("Title"; an empty title still follows the first line), then a compact row: Project (with suggestions, "No project"), a Pinned chip and the muted edit time ("Edited 21:39"). The writing area keeps the Markdown editor ("Write...  - [ ] makes a checklist").
- Each part saves by itself, like the task panel, and a failed save leaves the panel open with a message. A title that just follows the first line keeps following it when only the text is saved.
- Links (link picker and "Linked from") is one quiet collapsible section, folded by default; the device remembers it.
- A "..." menu in the panel header holds History, Pin / Unpin, Make a task and Delete note (with undo); the bottom History and Delete buttons are gone.
- "New note" in an empty view creates an empty note (in the current project) and opens it, instead of asking in a form first. A note left empty is dropped when the panel closes. The top-bar "New note" and quick capture with text keep their form, now with a borderless title and text.
- Row snippets show the text of Markdown links and images instead of `[text](url)`.
- `note` also returns `name` (the stored title) and `edited`. Pure UI and read-model change: no storage change; the operations are unchanged.

### Notes 0.5.1: a sidebar and calmer rows

- Notes now has a view sidebar like the task manager. **All notes**, **Pinned** and **Recent** (edited in the last 7 days) come first, then **Projects** (one entry each, with counts), then a folded **More** with **Task notes** and **Trash**. The project chips row is gone; the sidebar collapses to an icon rail (the toggle is next to the title), and the page title shows the current view or project.
- Rows are one calm line: the title, a muted snippet under it only when the note has text, and fixed columns on the right for a pin mark, checklist progress (2/5), the project (hidden inside a project or Task notes) and the edit date. A single ⋯ menu per row holds Pin / Unpin, Make a task and Delete (with undo). Rows are grouped Pinned / Notes with the group count; an empty view shows one short line and a "New note" button.
- Trash is a view in the sidebar instead of a section at the bottom of the page: the same rows, muted, with Restore in the ⋯ menu (and in the bulk bar).
- One counting rule: each sidebar count equals the rows its view shows. New queries `nav_counts` and `nav_projects`; `list` gains a last `view` parameter (empty keeps today's output); `projects` is unchanged. Pure UI and read-model change: no storage change.

### Task manager 0.12.4: the task panel reads like a task

- The task panel is no longer a long form. It opens with a round check and a big, borderless title (no "Title (required)" label; an empty title is still refused), then compact properties: List and Status, Plan for, Due and Estimate, Priority and Tags. Notes sit right under them in a borderless box that grows with your text ("Add notes").
- Below that, each part is a quiet collapsible heading: Subtasks (with its progress), Steps, Time, Fields and Dependencies. Subtasks and Steps are always open with an "Add a ..." line; Time, Fields and Dependencies start folded when empty and open when they have content. The device remembers a section you open or fold. "More" keeps Repeat, Reminder, Attention, Group, position, links and History.
- The check in the header replaces the old "Mark done" button at the bottom; the panel stays open after it. The Archive banner (Unarchive / Move to trash) is unchanged. Autosave, undo and toasts are unchanged; Priority moved out of "More", and Estimate out of "Time".
- Host: `sheet.size: "panel"` (460px, docked or overlay, full screen on phones), field `style` (`title`, `bare`, `compact`) and a round `checkbox`. Pure UI change: no storage or query change.

### Task manager 0.12.3: calmer sidebar, one counting rule

- Sidebar names: "Due / overdue" is now **Due** (its count is red) and "Ready to pick" is now **Anytime**: open tasks whose status is ready to pick up, unless planned for a later day. Keys, saved views and queries are unchanged; only labels moved (the saved-view filter menu uses the new names too).
- **Lists** starts with **Inbox** (the default list), so the section is never empty while you have tasks; Inbox has no rename menu. **More** (Done, Task notes, Archive, Trash) folds from its title and starts folded; the device remembers it.
- One counting rule: every open task counts, subtasks included. The sidebar counts (Inbox, Due, Upcoming, Anytime, All open, each list) now equal the rows their view shows, and a group heading counts the rows under it, so "Inbox 20" in the sidebar and an INBOX heading no longer disagree (0.12.2 counted top-level rows in the headings and All open).
- `nav_counts.open` and `nav_lists.open` include subtasks. `nav_lists` returns Inbox first and gains `icon`, `filter`, `list_id` and `nav` (which view a row opens). Pure read-model and UI change: no storage change.
- Host: collapsed icon rail shows a small dot for a non-zero count instead of a pill over the icon (hover for the number); the sidebar's Collapse button no longer sits on top of Settings in a short window.

### Task manager 0.12.2: subtasks nest under their parent

- In list views, subtasks now sit indented under their parent instead of as separate flat rows (and instead of being hidden). The parent row gets a small disclosure arrow, shown with its progress (for example `1/3`); the arrow collapses or expands the subtasks. Everything starts expanded, and a collapsed parent stays collapsed on this device. The row menu's "Show subtasks" became "Add subtask" (an inline field under the row), since the subtasks are already visible.
- Rows shown nested no longer carry the muted parent chip. A subtask whose parent is not in the view (My Day with only the subtask planned, a done parent in the Open view, a search hit) stays a normal row with the muted "↳ Parent" hint.
- Sort and Group keep a family together: the subtasks follow their parent inside the parent's group, whatever list, plan date or status they carry themselves. Group counts count top-level tasks only. Completing a parent still leaves its open subtasks in place.
- Boards (Kanban, Matrix) show a card for the parent with `1/3`; its subtasks are not separate cards unless their parent is not on the board.
- `workspace_tasks` takes a last parameter `nesting` (`tree` for lists, `fold` for boards; empty keeps the earlier rule, used by saved views) and returns `depth` (0 or 1) with each row; `parent_label` is empty for a row nested under its parent in `tree` or `fold` mode. Pure read-model and UI change: no storage change.

### Task manager 0.12.1: calmer task rows and layout

- Task rows follow a Things / Todoist look: a round checkbox whose ring shows priority (high red, medium orange, low muted), one-line title, and a single muted line under it only when there is something to say (notes icon, subtask progress, steps, a running timer, the parent task). Dates, time, tags and the list name sit in fixed columns on the right, so they line up from row to row; at most two tags or field chips show.
- "Blocked" is now a small lock icon (hover for what blocks the task) and the parent chip is muted and cut to 18 characters. Hovering a row tints it and shows a quiet "..." menu (Plan today, Remove from today, steps, subtasks) instead of swapping the row's meta for buttons. The list name appears only in views that mix lists (My Day, Due, Upcoming, All open), not inside a list.
- Section headers show their task count, rows default to the 44px comfortable height, and empty views say one short line with an "Add a task" button. The list column uses up to 880px of the page and is centred.
- Pure UI change: no storage, query or command changes.

### Task manager 0.12.0: Archive for completed tasks

- Completed tasks can be archived: they leave the lists and move to a new **Archive** section (Task manager sidebar, More, before Trash) but stay done, and still count in reviews, reports, time summaries and exports. The Archive is separate from Trash, which is for deleted tasks. Tasks hidden earlier by "Clear completed" become the initial Archive; nothing is backfilled because the marker is the existing `tasks.cleared_at`.
- The Archive section shows "14 done this month · 9h 20m tracked", a search box (title, notes, tags and the note on finishing), a list filter and an Auto-archive menu (Never, End of day, After 7 days, After 30 days). Rows are grouped Today, Yesterday, This week and by month, newest first (up to 500), with the list, note, time and subtask chips. Opening one shows a banner with **Unarchive** and **Move to trash**; Start timer, Add subtask and Mark done are hidden for an archived task. The task menu gains Archive (done tasks) and Unarchive.
- "Clear completed" is now **Archive completed** ("Archive all completed tasks? They stay in the Archive and still count in reviews and reports."), with Undo. The command keeps the name `clear_done` and gains the alias `archive_done`; both return `cleared`, `archived` and `at`.
- Archiving a task also archives its completed subtasks (open ones stay and show as top-level tasks); `unarchive {id}` brings the task and the subtasks archived with it back, `unarchive {at}` undoes a whole bulk archive. Each occurrence of a repeating task is its own row and is archived on its own.
- Auto-archive: a daily schedule (00:05) archives completed tasks finished before today (End of day) or more than 7 or 30 days ago. Choosing Never turns the schedule off. It is safe to run on two devices, and it only touches tasks that are not archived yet. It notifies when it archived something (if "Notify me when a run has something ready" is on). The plugin makes the schedule itself, so users are not asked to approve it again.
- New commands (also MCP tools): `archive`, `unarchive`, `archive_done`, `archive_settings`, and queries `archived` (for questions like "what did I finish in October"; `from`/`to` are local completion dates) and `archive_settings`. `tasks` and `workspace_tasks` take an optional `include_archived` (last parameter, off by default); `task` returns `archived` and `archived_label`. `search`, the search index and the counts still leave archived tasks out.
- Storage version 9 adds one column, `settings.auto_archive` (empty means Never), so nothing changes silently. Devices still on Task manager 0.11 (storage version 8) keep every change made by 0.12 devices pending, unapplied, until they update; nothing is lost. Update every device.
- Not included: the Trash is not emptied automatically (a 30-day purge is a follow-up).

### Task manager 0.11.0: time estimates and time tracking

- Tasks get a time estimate and tracked time (ClickUp-style). New `tasks.estimate_min` (minutes) and table `time_entries` (`task_id`, `started_at`, `ended_at`, `minutes`, `note`; trashed rather than removed, so entries can be restored). Storage version 8, additive.
- Estimates and durations accept `1h 30m`, `90m`, `1.5h`, `2 hours 15 minutes`, `1:30` or a bare number of minutes (`45`); anything else is refused with the accepted formats. Zero or more than 1,000 hours is refused.
- One timer runs per space: `start_timer` stops the running one first (its time is kept) in the same step, and one Undo puts both back. `stop_timer` stops whatever runs, from whichever device started it, and rounds to whole minutes with a minimum of 1. Finishing a task stops its timer ("Task done · timer stopped at 12m"); moving a task to the trash stops the timers of it and its subtasks; a done task can't start a timer. A repeating task's next occurrence keeps the estimate but starts with no time.
- A parent task's shown estimate and tracked time include its subtasks' (`1h 20m / 3h`; the panel says "Includes subtasks: 40m of 1h"). A running timer counts its elapsed time in every total whenever a view refreshes.
- The task panel has a Time section (after Fields, before Subtasks): the tracked time against the estimate (an "Over by 30m" chip in the danger tone), an Estimate box that saves as you type, Start timer / Stop timer, "Add time" (time, date, note) and the list of entries with delete (and Undo). Rows on the list, Kanban and Eisenhower views, and the Home Today widget, show a clock chip (`1h 20m / 3h`, danger tone when over) and a `● Running` chip; in My Day, Planned earlier, Due and Inbox on Today the same appears in the note line next to the steps. The Sort menu gains Time tracked and Estimate (a parent sorts by its rolled-up values). While a timer runs, the Task manager sidebar shows an entry with its elapsed time that opens the task.
- The timer does not tick: the panel and chips show the elapsed time as of the last refresh (the panel says "Running since 14:05 · 12m so far"); the app has no live-updating text or per-plugin status badge, and the app tile's badge stays the count of due tasks.
- New commands (also MCP tools): `set_estimate`, `start_timer`, `stop_timer`, `add_time`, `update_time_entry`, `delete_time_entry`, `restore_time_entry` (plus the hidden Undo helpers `resume_timer` and `undo_start_timer`), and queries `time_entries`, `running_timer` and `time_summary` (per task or per list between two dates, own entries only; for Reports). `task` and `tasks` return `estimate_min` and `tracked_min` (with the subtask rollup), `own_estimate_min` and `own_tracked_min` (`task`), `running`, and ready-made `time_label`, `time_tone` and `running_label`.
- The CSV export adds `estimate` and `tracked` columns (`1h 30m`, a task's own values so a spreadsheet can add them up); the space JSON export carries `time_entries` as stored. The CSV importers ignore both columns.
- Devices still on Task manager 0.10 (storage version 7) keep every change made by 0.11 devices pending, unapplied, until they update; nothing is lost. A timer started on one device is seen on the others through sync, and stopping it on any of them works. Update every device.

### Task manager 0.10.0: custom fields

- Tasks get custom fields (ClickUp-style): text, number, date, select, checkbox and url. A field belongs to one list or to all lists, and shows on every task in that scope. New tables `task_fields` (archived rather than deleted, like statuses) and `task_field_values` (delete-wins; one value per task and field, the newest row wins if two devices wrote one offline); `tasks.fields_updated_at` marks a task as changed so search indexes its new values. Storage version 7, additive.
- The task panel has a Fields section (after the basic fields, before Subtasks) with a control per type that saves on change: text, link and number inputs, a date picker, a select with the field's options, and a checkbox. Select, date and checkbox changes offer Undo. Add field and Manage fields sit under it.
- Fields are managed where lists, groups and statuses are: List settings (and Lists, groups, statuses and fields for all-lists fields) list a scope's fields, New field takes a name, type, "applies to" (This list or All lists), select options (one per line) and Show on task rows; Edit field renames, reorders, archives and edits select options (rename, color, add, remove). Removing a select option, or dropping it from a replaced option list, clears that value on every task. A field's type and list can't change.
- Fields marked Show on task rows appear as chips on list, Kanban and Eisenhower rows (`Budget: 1200`, a select shows its option label, a checkbox shows the field name when ticked, empty values show nothing), at most three per row, before the tags.
- Filter has a Field choice per select option and checkbox (`Stage: Design`): several options of one field match any of them, different fields must all match. Saved views keep it (`field_values`). Sort by a number or date field is not offered: the Sort menu takes a fixed list of options.
- Search (the ⌘K index, the fallback search and the page search box) matches text, link and select-label values; numbers, dates and checkboxes are not searched. A task's `fields` summary (`Budget: 1200 · Stage: Design`) is on the `task` query. The CSV export adds a `custom_fields` column with the same summary (export columns are fixed, so there is one column rather than one per field); the space JSON export carries both tables as stored.
- New commands (also MCP tools): `create_field`, `update_field`, `add_field_option`, `update_field_option`, `remove_field_option`, `set_field` (validates by type; returns the previous value; an empty value clears) and queries `fields` and `task_field_values`.
- Devices still on Task manager 0.9 (storage version 6) keep every change made by 0.10 devices pending, unapplied, until they update; nothing is lost. Update every device.

### Task manager 0.9.0: subtasks and dependencies

- Tasks can have subtasks, one level deep (new `tasks.parent_id`; storage version 6, additive). A subtask is a full task (due, priority, status, tags, reminders, repeat, plan, list) and joins its parent's list and group unless given others. A subtask can't get subtasks, and a task with subtasks can't become one.
- Lists, Kanban, Eisenhower, All open and list counts show top-level tasks; a parent row has a `2/5` progress chip and Show subtasks / Hide subtasks on hover, which expands its subtasks (indented) under the row. My Day, Due / overdue, Upcoming, Ready, Inbox, search, a tag or text filter, Today and the Home Today widget list subtasks as their own rows with a `↳ Parent` chip.
- The task panel has Subtasks (above Steps) with done circles, due chips and an Add subtask box; each subtask opens in the panel and shows a "Parent: …" link at the top. Steps get Make subtask (undo puts the step back). The panel menu has Detach from parent.
- Trashing a task trashes its subtasks; restoring it restores the ones trashed with it (marked by the new `trashed_with` column). A subtask whose parent is gone for good, or whose parent was cleared from the lists, shows as a top-level task. Finishing a parent doesn't finish its subtasks, and finishing the last subtask doesn't finish the parent. A repeating parent's next occurrence starts without subtasks.
- Dependencies ("Waiting on"): new `task_deps` table (delete-wins). A task is blocked while a task it waits on is not done and not in the trash; blocked rows show a lock "Blocked" chip (its tooltip names the blockers) and sort after unblocked ones in Today and My Day / Due / Ready. The panel's Dependencies section lists Waiting on and Blocking (open, remove with undo) and adds one through a title search that leaves out the task itself, existing blockers and anything that would make a loop. Finishing a blocked task is allowed: "Done — was waiting on 1 task".
- New commands (also MCP tools): `set_parent`, `promote_step`, `add_dependency`, `remove_dependency`, and queries `subtasks` and `dependencies`. `create` takes `parent_id`; `task` returns `parent_id`, `parent_title`, `subtask_count`, `subtask_done`, `blocked` and `blocked_by`. The CSV export adds `parent` and `waiting_on` (titles); the CSV importers ignore them. Space JSON export carries `parent_id` and `task_deps` as stored.
- Devices still on Task manager 0.8 (storage version 5) keep every change made by 0.9 devices pending, unapplied, until they update; nothing is lost. Update every device.
- The lock icon and text-chip tooltips (`label`) need the matching host renderer; older hosts show the "Blocked" text without them.
- Voice 0.2.7 says Task manager instead of To-do in its description, buttons and messages; the `to: "todo"` value is unchanged.

### To-do is now Task manager (`tasks`)

- The To-do plugin is renamed **Task manager** with plugin id `tasks` (package `packages/tasks`, crate `tasks`), version 0.8.0. Its manifest declares `"former_ids": ["todo"]`, so its database (`todo.db`), sync and backup ids stay `todo`: existing tasks, devices on older builds and rollback keep working. Hosts that do not know `former_ids` reject this manifest; ship it only with a host that does.
- Refs, links, page widgets, saved views and automations that name `todo` keep resolving to Task manager.
- Voice 0.2.6 opens sent captures in `tasks`.

### Today (`today-v1`)

- To-do adds My Day, planned-earlier, due and overdue tasks with reminders, the Inbox, and today's completed tasks to Today, with Tomorrow, Not today, Keep, Drop and Move all to today actions.
- Habits adds today's habits as check-in chips with streaks, plus an evening sleep prompt.
- Voice adds captures waiting for review to Today, with → Task, → Note and Dismiss actions.
- Bookmarks adds untagged links to Today's To sort section.
- Feeds adds unread articles to Today's Catch up section, with Mark read and Undo.

### Slim To-do page

- To-do has a collapsible view sidebar: smart views with counts (My Day, Inbox, Due / overdue, Upcoming, Ready to pick, All open), Lists with counts, + and a hover ⋯ menu, Saved views with + to save the current view, and Done, Task notes and Trash.
- The To-do head shows the current view's name with a ⋯ menu for selecting tasks, saving a view, managing lists and statuses, task note settings and Clear completed; help text moved to tooltips and empty states.
- To-do capture is one box with the destination inside; it follows the list you are viewing.
- To-do has one toolbar row: search, Filter with removable chips ("Temporary · Save view · Reset"), Sort, Group, List / Kanban / Eisenhower and Display (row height and which details rows show).
- To-do list rows are single lines (title, steps, tags, due, list) with Plan today and steps on hover; Kanban and Eisenhower cards are compact with actions on hover.
- To-do 0.7.2 adds `nav_counts` and `nav_lists` queries for the sidebar.

### To-do task details

- To-do task details save as you edit (a moment after typing and when you leave a field) with a small Saved / Saving… / error note; the Save button is gone.
- Task details show Plan, Due, List and status, Tags, Notes and Steps first; Priority, Repeat, Reminder, In progress, Urgent / Important, group and position, links and History sit under a "More" section that stays open or closed as you left it.
- Delete moved to the task panel's ⋯ menu (with History); Mark done stays in the panel.
- A task due today at a time that has already passed shows as overdue in Today, in To-do lists and in the Home Today widget.
- Tasks without a due date no longer show a "No date" chip in the list, Kanban, Eisenhower or the Today widget.
- Saving a Kanban or Eisenhower layout as a view now keeps it (new Stage and Quadrant view columns): Kanban reopens as a board and Eisenhower as the 2×2 matrix.
- To-do task details keep normal spacing between Plan / Due, List / Status and Tags.
- To-do 0.7.4.
- To-do task rows show their details in one quiet group: the due date and time first (`10 Dec · 2pm`), repeat as an icon, tags last; the list name is hidden inside that list's own group. To-do 0.7.5.
- To-do task rows planned for today show **Remove from today** instead of Plan today, so a plan can be undone from the list. To-do 0.7.5.

### Fixed in UI review round 2

- Unsaved Bookmarks and other quick-entry drafts can be kept while navigating between apps (host fix).
- Bookmarks updates the All count even when no project has been assigned.
- Feeds shows locale dates without times, distinguishes unread articles, omits repeated grouped sources, and uses article links, explicit read actions, one Refresh button and consistent filters.
- Bookmarks merges its count into the filters and shows project/tag save hints only when selected.
- Bookmarks and Reports display local times consistently.
- Reports and Voice show one empty message per section, and empty sleep charts explain that nothing is logged.
- To-do uses compact board actions, accurate filter help and an Unsorted matrix group.
- Today starts with its No date group collapsed and remembers expansion in the saved view.
- Habits provides titled choices for automation check-ins and the Gym example.
- Habits keeps completed titles readable and shows percentage units in insights.
- The Home habit widget explains how to add habits instead of showing an empty grid.
- Task search includes due dates, tags and a note excerpt to distinguish similar titles.
- New-task reminders stay blank until a due date is set.
- Notes consistently uses New note, and Voice uses New capture.

### Added

- **To-do 0.5.0:** indexed task search, item references and linked note
  context; viewable task data with board and matrix moves; bulk completion,
  updates, tags, trash and restore; CSV export and task import; item History.
  Real completions emit `task_done` for approved automations.
- **Notes 0.3.3:** indexed search, item chips, @mentions and Linked from;
  viewable note data; bulk project changes, trash and restore; Android text
  sharing; Markdown export and folder import; item History.
- **Bookmarks 0.2.3:** indexed search and item references, viewable bookmark
  data, bulk pin/unpin, trash and restore, and Android text/link sharing.
- **Voice 0.2.3:** indexed capture search and item references.
- **Habits 0.3.0:** search and item references. Effective check-in changes
  emit `checked_in` without notes; repeating the same day/status is harmless.
- **Reports 0.3.4:** search and item references for saved reports.
- **Feeds 0.1.0:** public HTTPS RSS/Atom subscriptions, conditional refresh,
  text summaries, read/unread controls and an unread widget. Keeps the newest
  200 items per feed plus older unread items, and refreshes up to 20 feeds
  per run. Hourly schedules default to desktop. Requires an HTTP-capable host
  and explicit **Reach any website** approval on each device, including for
  sites not added as feeds. Runs only while the app is open (foreground on
  Android); feed content never automatically loads remote images.
- Deterministic unsigned catalog generation for reviewed plugin artifacts;
  registry signing is a separate host CLI step.
- First public source preview of the original six plugins, release bundle
  builder with SDK verification and digest pinning, and a vendored SDK for
  `terrarium:plugin@0.1.0`.
- Command descriptions in every plugin manifest for command-line clients
  and agents: typed arguments, effects, confirmation questions, examples,
  item destinations and hidden internal commands.

### Changed

- **Voice 0.2.3 and Habits 0.3.0:** desktop instructions follow the input
  device. **Habits 0.3.0:** new reminders default to the device in use.

### Fixed

- **To-do 0.5.0:** completion says **Task done** and offers the optional note
  action in a toast; saving does not interrupt you with a note prompt.

Upgrade Terrarium on every paired device before installing these packages.
New manifest declarations need matching host support; event-enabled packages
also require reviewing automations on the chosen execution device.

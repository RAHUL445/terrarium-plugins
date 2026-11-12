# To-do workspace implementation

Branch: `feature/todo-organization`, based on `preview/integration`.

## Milestones

1. **M1a: additive storage foundation.** Add organization and planning fields;
   prove v4 records survive v5 migration. Preserve current UI and operations.
2. **M1b: organization commands.** Lists, groups, customizable statuses, validated
   task movement, completion compatibility, and recurrence inheritance.
3. **M2: capture and focus.** Inbox capture, inline organization, task details,
   intentional daily planning separate from deadlines, bulk actions.
4. **M3: shared host views.** Structured filters before pagination, dynamic
   choices, named views reused on pages and in To-do, explicit save semantics.
5. **M4: interaction polish.** Display preferences, keyboard/mobile paths,
   inline steps, undo, empty states, and workflow verification.

Each milestone is tested and committed independently. No merge, publication,
installation into the owner's app, or live-data migration is part of these
development commits.

## Data contract

- A task has at most one list and one organizational group. A group belongs to
  a list. Null list means Inbox; null group means Ungrouped. These implicit
  defaults avoid duplicate seed rows when devices upgrade independently.
- Status is separate from group. Status definitions belong to a list (null
  list means Inbox). Stable IDs are references; names and colors are editable.
- Custom status categories are `not_started`, `active`, and `completed`.
  A separate `ready` flag distinguishes Waiting/Blocked from ready-to-pick work.
  Cancellation, dependencies, and rich subtasks are deferred.
- Existing `done` and `in_progress` remain the compatibility completion state.
  Until a custom status is assigned, they project as To pick, In progress,
  or Done. New writes must maintain consistency; old UI operations cannot be
  allowed to leave a contradictory custom status visible.
- `previous_status_id` supports reopening. `planned_for` is a date selected
  for work and does not change `due` or reminders. `sort_order` is task order;
  it does not replace an explicit view sort.
- Missing/deleted or wrong-list references remain stored and are flagged in
  the organization query for repair. A friendly fallback must not erase the
  original ID or silently rewrite history.
- Groups/statuses can be archived. Commands must validate reassignment before
  destructive removal. Cross-list moves must validate both group and status.

## Migration and rollback

M1a is expansion only: schema 4 to 5 adds nullable task references, a nullable
planning date, a default-zero order, and three empty definition tables. No
existing task or step is rewritten. Legacy tasks immediately retain their
completion semantics and appear under Inbox / Ungrouped in the new read model.
Current task queries and the existing UI retain their contracts.

Upgrade paired clients before using organization writes. Mixed-version sync
of new fields is not claimed safe until explicitly tested. The host rejects
schema downgrade; rollback after real deployment requires restoring a pre-upgrade
data backup with the old package. Do not delete new columns to simulate rollback.
Development verification uses disposable stores only.

## Later behavior decisions already agreed in review

- Capture requires only a title. A multi-list filtered view must not guess a
  destination; show Inbox or let the user choose.
- OR within a filter, AND across fields. Tags offer any/all. Invalid saved
  references remain visible for repair rather than broadening the view.
- Host owns view definitions/layouts; plugins own data and write operations.
- Shared-view edits distinguish temporary changes, Save changes, and Save as
  new; page placement keeps its own size and position.
- Current checklist steps remain steps. Independently scheduled subtasks,
  dependencies, and richer repeat rules are later work.
- Completing a repeat keeps list/group, resets workflow/planning state, and
  preserves the existing recurrence retry guard and reset checklist behavior.

## Verification status

M1a passed: six To-do Rust unit tests, six organization-query behavior tests,
eleven import/export and bulk-action contract tests, and seven real host To-do
integration tests. The new v4-to-v5 migration test compares every original
task/step field after upgrade and verifies that a second load is unchanged.
The host test is on the matching `feature/todo-organization` branch.

Implementation and test outcomes are recorded with each milestone commit.
Native UI, mobile, multi-device sync, and user-data migration require separate
verification; passing schema/query tests does not establish those outcomes.

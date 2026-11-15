"""Subtasks and dependencies in the read model (FIX-16).

Lists, boards and groups show top-level tasks with a progress chip;
actionable views (My Day, Today, Inbox, search, Home) list subtasks as rows
with their parent's title. Blocked-ness is derived from task_deps."""
from organization_query import OrganizationQuery, MANIFEST

TODAY = '2026-11-12'


class StructureQuery(OrganizationQuery):
    def add(self, id, **values):
        values.setdefault('title', id)
        self.insert('tasks', id=id, **values)

    def dep(self, task, blocker, id=None):
        self.insert('task_deps', id=id or f'{task}>{blocker}', task_id=task, depends_on=blocker)

    def workspace(self, filter='all', q=None, tags=None, lists=None, page_focus=None):
        return self.query('workspace_tasks', TODAY, filter, q, tags, lists, None, None,
                          'any', None, 'manual', 'list', page_focus, None, None)

    def ids(self, rows):
        return [r['id'] for r in rows]

    def family(self):
        self.insert('task_lists', id='work', name='Work')
        self.add('launch', list_id='work')
        self.add('draft', parent_id='launch', list_id='work', planned_for=TODAY)
        self.add('review', parent_id='launch', list_id='work', done=1)
        self.add('ship', parent_id='launch', list_id='work')

    def test_storage_is_additive_since_version_6(self):
        storage = MANIFEST['storage']
        self.assertEqual(storage['version'], 9)  # 7 custom fields (FIX-17), 8 time tracking (FIX-18), 9 auto-archive (FIX-19)
        tasks = next(t for t in storage['tables'] if t['name'] == 'tasks')
        names = [c['name'] for c in tasks['columns']]
        self.assertEqual(names[-4:], ['parent_id', 'trashed_with', 'fields_updated_at', 'estimate_min'])
        deps = next(t for t in storage['tables'] if t['name'] == 'task_deps')
        self.assertEqual(deps['delete_policy'], 'delete-wins')
        self.assertEqual([c['name'] for c in deps['columns']], ['task_id', 'depends_on'])

    def test_list_views_hide_subtasks_and_count_progress(self):
        self.family()
        rows = {r['id']: r for r in self.workspace('all')}
        self.assertEqual(set(rows), {'launch'})
        self.assertEqual(rows['launch']['subtasks_label'], '1/3')
        self.assertEqual((rows['launch']['subtasks_done'], rows['launch']['subtasks_total']), (1, 3))
        self.assertIsNone(rows['launch']['parent_label'])
        # The list itself (sidebar list view) and Done also stay top-level.
        self.assertEqual(self.ids(self.workspace('all', lists='work')), ['launch'])
        self.assertEqual(self.ids(self.workspace('done')), [])
        # A task without subtasks shows no progress chip.
        self.add('solo')
        self.assertIsNone({r['id']: r for r in self.workspace('all')}['solo']['subtasks_label'])

    def test_actionable_views_list_subtasks_with_parent_title(self):
        self.family()
        my_day = self.workspace('my_day')
        self.assertEqual(self.ids(my_day), ['draft'])
        self.assertEqual(my_day[0]['parent_label'], '↳ launch')
        today = self.query('today_my_day', TODAY)
        self.assertEqual([(r['id'], r['parent_label']) for r in today], [('draft', '↳ launch')])
        # Searching or filtering by tag finds subtasks in any view.
        self.assertEqual(set(self.ids(self.workspace('all', q='ship'))), {'ship'})
        search = {r['id']: r for r in self.query('search', 'ship')}
        self.assertTrue(search['ship']['subtitle'].startswith('↳ launch · '))
        self.assertFalse(self.query('search', 'launch')[0]['subtitle'].startswith('↳'))
        # Home Today widget / MCP `tasks` list subtasks too, with the chip.
        home = {r['id']: r for r in self.query('tasks', TODAY, 'open')}
        self.assertEqual(set(home), {'launch', 'draft', 'ship'})
        self.assertEqual(home['ship']['parent_label'], '↳ launch')
        self.assertEqual(home['launch']['subtasks_label'], '1/3')

    def test_inbox_today_lists_subtasks_of_inbox_parents(self):
        self.add('errands')
        self.add('milk', parent_id='errands')
        rows = {r['id']: r for r in self.query('today_inbox', TODAY)}
        self.assertEqual(set(rows), {'errands', 'milk'})
        self.assertEqual(rows['milk']['parent_label'], '↳ errands')
        self.assertEqual(set(self.ids(self.workspace('inbox'))), {'errands', 'milk'})

    def test_sidebar_counts_follow_the_views(self):
        self.family()
        counts = self.query('nav_counts', TODAY)[0]
        # One rule everywhere: every open task counts, subtasks included.
        self.assertEqual((counts['open'], counts['my_day']), (3, 1))
        self.assertEqual(counts['open'], len(self.nest('tree', filter='all')))
        self.assertEqual(self.query('nav_lists')[0]['open'], counts['inbox'])
        # Reviews and summaries still count every task.
        self.assertEqual(self.query('counts', TODAY)[0]['open'], 3)

    def test_orphans_are_top_level(self):
        self.add('kept', parent_id='gone')  # parent removed for good
        self.add('trashed-parent', _deleted='hlc')
        self.add('loose', parent_id='trashed-parent')  # restored on its own
        self.add('cleared-parent', done=1, cleared_at=1)
        self.add('open-child', parent_id='cleared-parent')
        rows = {r['id']: r for r in self.workspace('all')}
        self.assertEqual(set(rows), {'kept', 'loose', 'open-child'})
        self.assertTrue(all(r['parent_label'] is None for r in rows.values()))
        task = {r['id']: r for r in [self.query('task', i)[0] for i in ['kept', 'loose', 'open-child']]}
        # Gone parent: a plain task again. Trashed parent: still its subtask.
        self.assertEqual((task['kept']['can_have_subtasks'], task['kept']['parent_in_trash']), (1, 0))
        self.assertEqual((task['loose']['can_have_subtasks'], task['loose']['parent_in_trash']), (0, 1))
        self.assertIsNone(task['loose']['parent_title'])
        self.assertEqual(task['open-child']['parent_title'], 'cleared-parent')

    def test_task_query_describes_structure(self):
        self.family()
        self.add('budget')
        self.add('legal', done=1)
        self.dep('launch', 'budget')
        self.dep('launch', 'legal')
        self.dep('ship', 'launch')
        parent = self.query('task', 'launch')[0]
        self.assertEqual((parent['parent_id'], parent['parent_title']), (None, None))
        self.assertEqual((parent['subtask_count'], parent['subtask_done'], parent['subtasks_title']), (3, 1, 'Subtasks · 1/3'))
        self.assertEqual((parent['blocked'], parent['blocked_by']), (1, 'budget'))
        self.assertEqual((parent['waiting_count'], parent['blocking_count'], parent['can_have_subtasks']), (2, 1, 1))
        child = self.query('task', 'ship')[0]
        self.assertEqual((child['parent_id'], child['parent_title'], child['can_have_subtasks']), ('launch', 'launch', 0))
        self.assertEqual(child['blocked_by'], 'launch')

    def test_blocked_ignores_done_and_trashed_blockers(self):
        self.add('task', planned_for=TODAY)
        self.add('open-blocker')
        self.add('done-blocker', done=1)
        self.add('trashed-blocker', _deleted='hlc')
        self.dep('task', 'done-blocker')
        self.dep('task', 'trashed-blocker')
        self.assertEqual(self.query('task', 'task')[0]['blocked'], 0)
        self.assertIsNone(self.query('today_my_day', TODAY)[0]['blocked_label'])
        self.dep('task', 'open-blocker')
        self.dep('task', 'open-blocker', id='duplicate-from-another-device')
        row = self.query('task', 'task')[0]
        self.assertEqual((row['blocked'], row['blocked_by']), (1, 'open-blocker'))
        self.assertEqual(self.query('today_my_day', TODAY)[0]['blocked_label'], 'Blocked')
        ws = {r['id']: r for r in self.workspace('all')}
        self.assertEqual((ws['task']['blocked_label'], ws['task']['blocked_by']), ('Blocked', 'open-blocker'))
        # Restoring the trashed blocker brings it back into effect.
        self.db.execute("UPDATE tasks SET _deleted = NULL WHERE id = 'trashed-blocker'")
        self.assertEqual(self.query('task', 'task')[0]['blocked_by'], 'open-blocker, trashed-blocker')
        # A finished task shows no Blocked chip.
        self.db.execute("UPDATE tasks SET done = 1 WHERE id = 'task'")
        self.assertIsNone({r['id']: r for r in self.workspace('everything')}['task']['blocked_label'])

    def test_blocked_tasks_sort_after_unblocked_in_today(self):
        for id in ['a', 'b', 'c']:
            self.add(id, planned_for=TODAY, due='2026-11-01')
        self.add('blocker')
        self.dep('c', 'blocker', id='1')
        self.dep('a', 'blocker', id='2')
        self.assertEqual(self.ids(self.query('today_my_day', TODAY)), ['b', 'c', 'a'])
        self.db.execute("UPDATE tasks SET planned_for = NULL")
        self.assertEqual(self.ids(self.query('today_due', TODAY)), ['b', 'c', 'a'])
        self.db.execute("UPDATE tasks SET planned_for = ?", [TODAY])
        self.assertEqual(self.ids(self.workspace('my_day'))[-2:], ['c', 'a'])

    def test_dependencies_both_directions_and_choices(self):
        for id in ['design', 'build', 'test', 'other', 'gone']:
            self.add(id)
        self.db.execute("UPDATE tasks SET _deleted = 'hlc' WHERE id = 'gone'")
        self.dep('build', 'design')
        self.dep('test', 'build')
        self.dep('build', 'gone')
        rows = self.query('dependencies', 'build', '')
        self.assertEqual([(r['direction'], r['id']) for r in rows], [('waiting_on', 'design'), ('blocking', 'test')])
        self.assertEqual((rows[0]['blocked_id'], rows[0]['blocker_id']), ('build', 'design'))
        self.assertEqual([r['id'] for r in self.query('dependencies', 'build', 'blocking')], ['test'])
        # Picker: by title, never itself, an existing blocker, or a loop
        # (test waits on build, so build can't wait on test).
        self.assertEqual(self.ids(self.query('task_choices', 'build', 't')), ['other'])
        self.assertEqual(self.ids(self.query('task_choices', 'build', '')), [])
        self.assertEqual(self.ids(self.query('task_choices', 'design', 'test')), [])
        self.assertEqual(self.query('task_choices', 'build', 'oth')[0]['for_id'], 'build')

    def test_subtasks_query_orders_open_first(self):
        self.family()
        self.db.execute("UPDATE tasks SET due = ? WHERE id = 'ship'", [TODAY])
        rows = self.query('subtasks', 'launch', TODAY)
        self.assertEqual(self.ids(rows), ['draft', 'ship', 'review'])
        self.assertEqual((rows[1]['due_label'], rows[1]['due_tone']), ('Today', 'accent'))

    def test_steps_of_a_subtask_cannot_be_promoted(self):
        self.family()
        self.insert('steps', id='s1', task_id='launch', title='Outline')
        self.insert('steps', id='s2', task_id='draft', title='Intro')
        self.assertEqual(self.query('steps', 'launch')[0]['can_promote'], 1)
        self.assertEqual(self.query('steps', 'draft')[0]['can_promote'], 0)

    def test_export_names_parent_and_blockers(self):
        self.family()
        self.add('budget')
        self.dep('ship', 'budget')
        rows = {r['title']: r for r in self.query('export_tasks')}
        self.assertEqual((rows['ship']['parent'], rows['ship']['waiting_on']), ('launch', 'budget'))
        self.assertIsNone(rows['launch']['parent'])
        csv = next(e for e in MANIFEST['export'] if e['format'] == 'csv')
        self.assertEqual(csv['columns'][-5:], ['parent', 'waiting_on', 'custom_fields', 'estimate', 'tracked'])
        # The full CSV importer maps no structure columns, so old and new
        # CSV files import alike (extra columns are ignored).
        importer = next(i for i in MANIFEST['import'] if i['id'] == 'tasks_csv')
        self.assertNotIn('parent', importer['fields'].values())

    def test_structure_commands_are_declared(self):
        ops = MANIFEST['operations']
        for name in ['set_parent', 'promote_step', 'add_dependency', 'remove_dependency']:
            self.assertTrue(ops[name]['description'])
            self.assertNotEqual(ops[name].get('cli'), False)
        self.assertEqual(ops['create']['params']['parent_id']['type'], 'string')
        self.assertEqual(ops['set_parent']['params']['parent_id']['type'], 'string')
        for name in ['subtasks', 'dependencies']:
            self.assertNotEqual(MANIFEST['queries'][name].get('cli'), False)

    # ---- nested rows (FIX-21): parent first, then its subtasks, with depth ----

    def nest(self, nesting='tree', filter='all', sort='manual', grouping='list', **named):
        names = MANIFEST['queries']['workspace_tasks']['params']
        params = [None] * len(names)
        values = {'today': TODAY, 'filter': filter, 'tags_mode': 'any', 'sort': sort,
                  'grouping': grouping, 'nesting': nesting, **named}
        for key, value in values.items():
            params[names.index(key)] = value
        return self.query('workspace_tasks', *params)

    def test_tree_lists_subtasks_right_after_their_parent_with_depth(self):
        self.family()
        self.add('solo', sort_order=2, list_id='work')
        rows = self.nest('tree', filter='everything', lists='work')
        ids = self.ids(rows)
        self.assertEqual(ids[0], 'launch')
        self.assertEqual(set(ids[1:4]), {'draft', 'review', 'ship'})
        self.assertEqual(ids[4], 'solo')
        self.assertEqual([r['depth'] for r in rows], [0, 1, 1, 1, 0])
        # Shown nested: no parent chip, but the parent keeps its progress.
        self.assertTrue(all(r['parent_label'] is None for r in rows))
        self.assertEqual(rows[0]['subtasks_label'], '1/3')

    def test_tree_open_subtask_stays_when_parent_is_done(self):
        # Completing a parent does not hide open subtasks: the open filter
        # leaves the parent out, so the subtask is a plain row with its hint.
        self.family()
        self.db.execute("UPDATE tasks SET done = 1 WHERE id = 'launch'")
        rows = {r['id']: r for r in self.nest('tree', filter='open')}
        self.assertEqual(set(rows), {'draft', 'ship'})
        self.assertEqual((rows['draft']['depth'], rows['draft']['parent_label']), (0, '↳ launch'))
        everything = self.nest('tree', filter='everything')
        self.assertEqual([r['depth'] for r in everything], [0, 1, 1, 1])

    def test_tree_parent_outside_the_view_keeps_the_hint(self):
        self.family()
        rows = self.nest('tree', filter='my_day')
        self.assertEqual(self.ids(rows), ['draft'])
        self.assertEqual((rows[0]['depth'], rows[0]['parent_label']), (0, '↳ launch'))
        # Plan the parent too and the subtask nests under it, without the hint.
        self.db.execute("UPDATE tasks SET planned_for = ? WHERE id = 'launch'", [TODAY])
        rows = self.nest('tree', filter='my_day')
        self.assertEqual(self.ids(rows), ['launch', 'draft'])
        self.assertEqual([r['depth'] for r in rows], [0, 1])
        self.assertIsNone(rows[1]['parent_label'])

    def test_tree_sort_moves_children_with_their_parent(self):
        self.insert('task_lists', id='work', name='Work')
        self.add('b-parent', title='Bravo', list_id='work', priority=1)
        self.add('b-kid', title='Zulu kid', parent_id='b-parent', list_id='work', priority=3)
        self.add('a-parent', title='Alpha', list_id='work', priority=2)
        self.add('a-kid', title='Yankee kid', parent_id='a-parent', list_id='work')
        self.add('c-solo', title='Charlie', list_id='work', priority=3)
        by_title = self.nest('tree', sort='title')
        self.assertEqual(self.ids(by_title), ['a-parent', 'a-kid', 'b-parent', 'b-kid', 'c-solo'])
        by_priority = self.nest('tree', sort='priority')
        self.assertEqual(self.ids(by_priority), ['c-solo', 'a-parent', 'a-kid', 'b-parent', 'b-kid'])
        self.assertEqual([r['depth'] for r in by_priority], [0, 0, 1, 0, 1])

    def test_tree_group_keeps_children_in_the_parents_group(self):
        self.insert('task_lists', id='work', name='Work')
        self.insert('task_lists', id='home', name='Home')
        self.add('p', list_id='work', planned_for=TODAY)
        # The subtask was planned earlier and sits in another list: it still
        # follows its parent's group heading.
        self.add('k', parent_id='p', list_id='home', planned_for='2026-11-01')
        self.add('q', list_id='home', planned_for='2026-11-01')
        rows = {r['id']: r for r in self.nest('tree', filter='my_day', grouping='plan')}
        self.assertEqual(rows['k']['workspace_group'], rows['p']['workspace_group'])
        self.assertEqual(rows['k']['workspace_group'], 'Planned today')
        by_list = {r['id']: r for r in self.nest('tree', grouping='list')}
        self.assertEqual(by_list['k']['workspace_group'], 'Work')
        self.assertEqual(by_list['q']['workspace_group'], 'Home')
        ordered = self.ids(self.nest('tree', grouping='list'))
        self.assertEqual(ordered.index('k'), ordered.index('p') + 1)

    def test_fold_leaves_out_subtasks_whose_parent_is_listed(self):
        self.family()
        self.assertEqual(self.ids(self.nest('fold', filter='all')), ['launch'])
        # Parent not in the view: the subtask is its own card.
        self.assertEqual(self.ids(self.nest('fold', filter='my_day')), ['draft'])

    def test_without_nesting_the_older_rules_apply(self):
        self.family()
        self.assertEqual(self.ids(self.nest(None, filter='all')), ['launch'])
        rows = self.nest(None, filter='my_day')
        self.assertEqual((self.ids(rows), rows[0]['parent_label'], rows[0]['depth']), (['draft'], '↳ launch', 0))
        self.db.execute("UPDATE tasks SET planned_for = ? WHERE id = 'launch'", [TODAY])
        rows = self.nest(None, filter='my_day')
        self.assertEqual([r['depth'] for r in rows], [0, 0])
        self.assertEqual(rows[1]['parent_label'] if rows[1]['id'] == 'draft' else rows[0]['parent_label'], '↳ launch')

    def test_nesting_is_declared_last(self):
        query = MANIFEST['queries']['workspace_tasks']
        self.assertEqual(query['params'][-1], 'nesting')
        self.assertEqual(query['types']['nesting']['type'], 'string')


if __name__ == '__main__':
    import unittest
    unittest.main()

"""Custom fields in the read model (FIX-17).

Fields belong to one list or to all lists; each task has at most one value
per field (the newest live row wins if two devices wrote one). The task panel,
row chips, search, filters, the `task` summary and the CSV export all read
the same two tables."""
import json
from organization_query import OrganizationQuery, MANIFEST

TODAY = '2026-11-12'
STAGE = json.dumps([{'id': 'd', 'label': 'Design'}, {'id': 'b', 'label': 'Build', 'color': '#336699'}, {'id': 'x', 'label': 'Done'}])


class FieldsQuery(OrganizationQuery):
    def task(self, id, **values):
        values.setdefault('title', id)
        self.insert('tasks', id=id, **values)

    def field(self, id, name, type='text', list_id=None, options=None, show=0, position=0, archived=0, **extra):
        self.insert('task_fields', id=id, name=name, type=type, list_id=list_id, options=options,
                    show_on_row=show, position=position, archived=archived, **extra)

    def value(self, task, field, value, id=None, **extra):
        self.insert('task_field_values', id=id or f'{task}.{field}', task_id=task, field_id=field, value=value, **extra)

    def workspace(self, filter='all', q=None, fields=None, **_):
        return self.query('workspace_tasks', TODAY, filter, q, None, None, None, None, 'any', None, 'manual', 'list',
                          None, None, None, None, fields)

    def rows(self, **kw):
        return {r['id']: r for r in self.workspace(**kw)}

    def values(self, task, today=None):
        return {r['name']: r for r in self.query('task_field_values', task, today)}

    # ---- schema -----------------------------------------------------------

    def test_storage_adds_two_tables_in_version_7(self):
        storage = MANIFEST['storage']
        self.assertGreaterEqual(storage['version'], 7)  # 7 added the field tables; 8 is time tracking (FIX-18)
        fields = next(t for t in storage['tables'] if t['name'] == 'task_fields')
        self.assertEqual([c['name'] for c in fields['columns']],
                         ['list_id', 'name', 'type', 'options', 'show_on_row', 'position', 'archived'])
        self.assertNotIn('delete_policy', fields)  # archived, never deleted (like statuses)
        show = next(c for c in fields['columns'] if c['name'] == 'show_on_row')
        self.assertEqual((show['type'], show['default']), ('boolean', False))
        values = next(t for t in storage['tables'] if t['name'] == 'task_field_values')
        self.assertEqual(values['delete_policy'], 'delete-wins')
        self.assertEqual([c['name'] for c in values['columns']], ['task_id', 'field_id', 'value'])
        self.assertEqual(MANIFEST['version'], '0.12.4')

    # ---- applicable fields ------------------------------------------------

    def test_fields_for_a_list_are_its_own_plus_all_lists_by_position(self):
        self.insert('task_lists', id='work', name='Work')
        self.insert('task_lists', id='home', name='Home')
        self.field('everywhere', 'Owner', position=2)
        self.field('mine', 'Budget', 'number', list_id='work', position=1)
        self.field('theirs', 'Rooms', 'number', list_id='home', position=0)
        self.field('old', 'Legacy', list_id='work', position=0, archived=1)
        self.field('gone', 'Trashed', position=0, _deleted='hlc')
        work = self.query('fields', 'work')
        self.assertEqual([r['name'] for r in work], ['Budget', 'Owner'])
        self.assertEqual([r['scope_label'] for r in work], ['Work', 'All lists'])
        self.assertEqual([r['name'] for r in self.query('fields', 'home')], ['Rooms', 'Owner'])
        # Inbox (no list) only has the all-lists fields; so does an unknown list.
        self.assertEqual([r['name'] for r in self.query('fields', '')], ['Owner'])
        self.assertEqual([r['name'] for r in self.query('fields', None)], ['Owner'])
        self.assertEqual([r['name'] for r in self.query('fields', 'nope')], ['Owner'])
        # Same position: by name.
        self.field('a', 'Alpha', position=2)
        self.assertEqual([r['name'] for r in self.query('fields', '')], ['Alpha', 'Owner'])

    def test_empty_list_id_and_null_both_mean_all_lists(self):
        self.field('blank', 'Blank', list_id='')
        self.field('null', 'Null', list_id=None, position=1)
        self.task('t')
        self.assertEqual(set(self.values('t')), {'Blank', 'Null'})

    def test_task_field_values_lists_every_applicable_field(self):
        self.insert('task_lists', id='work', name='Work')
        self.field('f_text', 'Notes', 'text', position=0)
        self.field('f_num', 'Budget', 'number', list_id='work', position=1)
        self.field('f_date', 'Review', 'date', position=2)
        self.field('f_sel', 'Stage', 'select', options=STAGE, position=3)
        self.field('f_chk', 'Billable', 'checkbox', position=4)
        self.field('f_url', 'Spec', 'url', position=5)
        self.field('f_other', 'Other list', list_id='home', position=6)
        self.field('f_old', 'Archived', position=7, archived=1)
        self.task('t', list_id='work')
        self.task('inbox')
        self.value('t', 'f_text', 'hello')
        self.value('t', 'f_num', '1200')
        self.value('t', 'f_date', '2026-12-03')
        self.value('t', 'f_sel', 'b')
        self.value('t', 'f_chk', '1')
        self.value('t', 'f_url', 'https://example.com/spec')
        self.value('t', 'f_old', 'hidden')
        rows = self.query('task_field_values', 't')
        self.assertEqual([r['name'] for r in rows], ['Notes', 'Budget', 'Review', 'Stage', 'Billable', 'Spec'])
        by = {r['name']: r for r in rows}
        self.assertEqual((by['Budget']['value'], by['Budget']['display'], by['Budget']['type']), ('1200', '1200', 'number'))
        self.assertEqual((by['Stage']['value'], by['Stage']['display']), ('b', 'Build'))
        self.assertEqual((by['Billable']['checked'], by['Billable']['display']), (1, 'Yes'))
        self.assertEqual(by['Spec']['display'], 'https://example.com/spec')
        self.assertEqual(by['Review']['display'], '3 Dec 2026')
        self.assertEqual(json.loads(by['Stage']['options'])[1]['color'], '#336699')
        self.assertTrue(all(r['has_value'] == 1 for r in rows))
        # The other task (Inbox) sees the all-lists fields with no values.
        inbox = self.query('task_field_values', 'inbox')
        self.assertEqual([r['name'] for r in inbox], ['Notes', 'Review', 'Stage', 'Billable', 'Spec'])
        self.assertTrue(all(r['value'] == '' and r['has_value'] == 0 and r['checked'] == 0 for r in inbox))
        self.assertEqual(self.query('task_field_values', 'missing'), [])

    def test_dates_drop_the_year_when_it_is_the_current_one(self):
        self.field('d', 'Review', 'date')
        self.task('t')
        self.value('t', 'd', '2026-12-03')
        self.assertEqual(self.values('t', TODAY)['Review']['display'], '3 Dec')
        self.assertEqual(self.values('t', '2027-01-01')['Review']['display'], '3 Dec 2026')

    def test_unknown_option_and_trashed_values_read_as_empty(self):
        self.field('s', 'Stage', 'select', options=STAGE)
        self.task('a')
        self.task('b')
        self.value('a', 's', 'removed-option')
        self.value('b', 's', 'd', _deleted='hlc')
        self.assertEqual(self.values('a')['Stage']['display'], '')
        self.assertEqual(self.values('a')['Stage']['has_value'], 0)
        self.assertEqual(self.values('b')['Stage']['value'], '')
        # A broken options column doesn't break the query.
        self.field('bad', 'Broken', 'select', options='not json', position=1)
        self.assertEqual(self.values('a')['Broken']['display'], '')

    def test_newest_value_wins_when_devices_wrote_duplicates(self):
        self.field('n', 'Budget', 'number')
        self.field('s', 'Stage', 'select', options=STAGE, show=1)
        self.task('t')
        self.value('t', 'n', '10', id='0001')
        self.value('t', 'n', '25', id='0003')
        self.value('t', 'n', '99', id='0002')
        self.value('t', 's', 'd', id='0001s')
        self.value('t', 's', 'b', id='0002s')
        self.value('t', 's', 'x', id='0003s', _deleted='hlc')  # newest is gone: the live one counts
        values = self.values('t')
        self.assertEqual(values['Budget']['value'], '25')
        self.assertEqual(values['Stage']['value'], 'b')
        self.assertEqual(len(self.query('task_field_values', 't')), 2, 'one row per field')
        self.assertEqual(self.rows()['t']['field_chip_1'], 'Stage: Build')
        self.assertEqual(self.query('task', 't')[0]['fields'], 'Budget: 25 · Stage: Build')

    # ---- rows -------------------------------------------------------------

    def test_row_chips_use_label_value_and_hide_when_empty(self):
        self.field('n', 'Budget', 'number', show=1, position=0)
        self.field('s', 'Stage', 'select', options=STAGE, show=1, position=1)
        self.field('c', 'Billable', 'checkbox', show=1, position=2)
        self.field('d', 'Review', 'date', show=1, position=3)
        self.field('hidden', 'Quiet', 'text', show=0, position=4)
        self.task('full')
        self.task('empty')
        self.task('unticked')
        self.value('full', 'n', '1200')
        self.value('full', 's', 'd')
        self.value('full', 'c', '1')
        self.value('full', 'd', '2026-12-03')
        self.value('full', 'hidden', 'secret')
        self.value('unticked', 'c', '')
        rows = self.rows()
        self.assertEqual([rows['full'][f'field_chip_{n}'] for n in (1, 2, 3)], ['Budget: 1200', 'Stage: Design', 'Billable'])
        self.assertEqual([rows['empty'][f'field_chip_{n}'] for n in (1, 2, 3)], [None, None, None])
        self.assertEqual(rows['unticked']['field_chip_1'], None)
        # Only three chips per row, in field order; later values wait.
        self.assertNotIn('field_chip_4', rows['full'])
        # Empty fields don't leave gaps: the next one moves up.
        self.value('empty', 'd', '2026-11-12')
        self.value('empty', 'n', '7')
        chips = [self.rows()['empty'][f'field_chip_{n}'] for n in (1, 2, 3)]
        self.assertEqual(chips, ['Budget: 7', 'Review: 12 Nov', None])

    def test_row_chips_respect_scope_and_archive(self):
        self.insert('task_lists', id='work', name='Work')
        self.field('w', 'Budget', 'number', list_id='work', show=1)
        self.field('a', 'Old', 'text', show=1, archived=1, position=1)
        self.field('all', 'Owner', 'text', show=1, position=2)
        self.task('in_work', list_id='work')
        self.task('in_inbox')
        for t in ('in_work', 'in_inbox'):
            self.value(t, 'w', '5')
            self.value(t, 'a', 'x')
            self.value(t, 'all', 'me')
        rows = self.rows()
        self.assertEqual([rows['in_work']['field_chip_1'], rows['in_work']['field_chip_2']], ['Budget: 5', 'Owner: me'])
        self.assertEqual([rows['in_inbox']['field_chip_1'], rows['in_inbox']['field_chip_2']], ['Owner: me', None])

    def test_row_dates_drop_the_current_year(self):
        self.field('d', 'Review', 'date', show=1)
        self.task('t')
        self.value('t', 'd', '2027-02-01')
        self.assertEqual(self.rows()['t']['field_chip_1'], 'Review: 1 Feb 2027')

    # ---- search -----------------------------------------------------------

    def test_search_matches_text_url_and_select_values(self):
        self.field('t', 'Client', 'text')
        self.field('u', 'Spec', 'url')
        self.field('s', 'Stage', 'select', options=STAGE)
        self.field('n', 'Budget', 'number')
        self.field('c', 'Billable', 'checkbox')
        self.field('old', 'Legacy', 'text', archived=1)
        for t in ('a', 'b', 'c', 'd', 'e', 'f', 'plain'):
            self.task(t)
        self.value('a', 't', 'Globex Corporation')
        self.value('b', 'u', 'https://wiki.example.com/launch')
        self.value('c', 's', 'b')
        self.value('d', 'n', '777')
        self.value('e', 'c', '1')
        self.value('f', 'old', 'Initech')
        for q, want in [('globex', {'a'}), ('wiki.example', {'b'}), ('build', {'c'}), ('777', set()),
                        ('Yes', set()), ('Billable', set()), ('initech', set())]:
            self.assertEqual(set(self.rows(q=q)), want, q)
            self.assertEqual({r['id'] for r in self.query('search', q)}, want, q)
        # Only the option's label is searchable text, not its id.
        self.field('ids', 'Phase', 'select', options=json.dumps([{'id': 'zq9', 'label': 'Rollout'}]))
        self.value('plain', 'ids', 'zq9')
        self.assertEqual(set(self.rows(q='zq9')), set())
        self.assertEqual(set(self.rows(q='rollout')), {'plain'})
        # The host's index sees the same words as keywords, after the tags.
        self.task('tagged', tags='ops')
        self.value('tagged', 't', 'Globex')
        self.db.execute("UPDATE tasks SET notes = 'n' WHERE id = 'tagged'")
        index = {r['id']: r for r in self.query('index_tasks', json.dumps(['a', 'c', 'tagged', 'plain', 'd']))}
        self.assertEqual(index['a']['keywords'], 'Globex Corporation')
        self.assertEqual(index['c']['keywords'], 'Build')
        self.assertEqual(index['tagged']['keywords'], 'ops Globex')
        self.assertEqual(index['plain']['keywords'], 'Rollout')
        self.assertEqual(index['d']['keywords'], '', 'numbers are not keywords')

    def test_search_by_field_value_finds_subtasks_and_ignores_scope_mismatch(self):
        self.insert('task_lists', id='work', name='Work')
        self.field('w', 'Client', 'text', list_id='work')
        self.task('moved')  # now in Inbox but kept a value from its old list
        self.value('moved', 'w', 'Umbrella')
        self.assertEqual(set(self.rows(q='umbrella')), set())

    # ---- filter -----------------------------------------------------------

    def test_filter_equals_select_and_checkbox_choices(self):
        self.field('s', 'Stage', 'select', options=STAGE, position=0)
        self.field('c', 'Billable', 'checkbox', position=1)
        self.task('design')
        self.task('build_billable')
        self.task('build')
        self.task('none')
        self.value('design', 's', 'd')
        self.value('build_billable', 's', 'b')
        self.value('build_billable', 'c', '1')
        self.value('build', 's', 'b')
        self.assertEqual(set(self.rows(fields='s=d')), {'design'})
        self.assertEqual(set(self.rows(fields='s=b')), {'build_billable', 'build'})
        # Several choices of one field: any of them. Across fields: all.
        self.assertEqual(set(self.rows(fields='s=d,s=b')), {'design', 'build_billable', 'build'})
        self.assertEqual(set(self.rows(fields='s=b,c=1')), {'build_billable'})
        self.assertEqual(set(self.rows(fields='c=1')), {'build_billable'})
        self.assertEqual(set(self.rows(fields='')), {'design', 'build_billable', 'build', 'none'})
        self.assertEqual(set(self.rows(fields='s=nope')), set())
        # It composes with the other filters and ignores a trashed value.
        self.value('none', 's', 'd', id='zz', _deleted='hlc')
        self.assertEqual(set(self.rows(fields='s=d')), {'design'})
        self.assertEqual(set(self.rows(fields='s=b', q='billable')), {'build_billable'})
        self.assertEqual(set(self.rows(fields='s=d', q='billable')), set())

    def test_filter_uses_the_newest_value(self):
        self.field('s', 'Stage', 'select', options=STAGE)
        self.task('t')
        self.value('t', 's', 'd', id='01')
        self.value('t', 's', 'b', id='02')
        self.assertEqual(set(self.rows(fields='s=d')), set())
        self.assertEqual(set(self.rows(fields='s=b')), {'t'})

    def test_filter_choices_list_each_select_option_and_checkbox(self):
        self.field('s', 'Stage', 'select', options=STAGE, position=1)
        self.field('c', 'Billable', 'checkbox', position=0)
        self.field('n', 'Budget', 'number', position=2)
        self.field('old', 'Legacy', 'select', options=STAGE, archived=1)
        rows = self.query('field_filter_choices')
        self.assertEqual([(r['value'], r['label']) for r in rows],
                         [('c=1', 'Billable: Yes'), ('s=d', 'Stage: Design'), ('s=b', 'Stage: Build'), ('s=x', 'Stage: Done')])

    # ---- panel and management helpers -------------------------------------

    def test_task_summary_and_scope(self):
        self.insert('task_lists', id='work', name='Work')
        self.field('n', 'Budget', 'number', position=0)
        self.field('s', 'Stage', 'select', options=STAGE, position=1)
        self.field('c', 'Billable', 'checkbox', position=2)
        self.field('t', 'Client', 'text', position=3)
        self.task('t1', list_id='work')
        self.task('t2')
        self.value('t1', 'n', '1200')
        self.value('t1', 's', 'd')
        self.value('t1', 'c', '1')
        row = self.query('task', 't1')[0]
        self.assertEqual(row['fields'], 'Budget: 1200 · Stage: Design · Billable')
        self.assertEqual(row['field_scope'], 'work')
        empty = self.query('task', 't2')[0]
        self.assertEqual((empty['fields'], empty['field_scope']), ('', '@inbox'))

    def test_field_choices_follow_option_order(self):
        self.field('s', 'Stage', 'select', options=STAGE)
        self.field('t', 'Notes', 'text')
        rows = self.query('field_choices', 's')
        self.assertEqual([(r['value'], r['label'], r['color']) for r in rows],
                         [('d', 'Design', ''), ('b', 'Build', '#336699'), ('x', 'Done', '')])
        self.assertEqual(self.query('field_choices', 't'), [])
        self.assertEqual(self.query('field_choices', 'missing'), [])

    def test_field_defs_scope_detail_and_choices_for_the_manage_sheets(self):
        self.insert('task_lists', id='work', name='Work')
        self.field('mine', 'Budget', 'number', list_id='work', show=1)
        self.field('arch', 'Old', 'text', list_id='work', archived=1, position=5)
        self.field('sel', 'Stage', 'select', options=STAGE, position=1)
        work = self.query('field_defs', 'work')
        self.assertEqual([r['name'] for r in work], ['Budget', 'Old'])
        self.assertEqual(work[0]['summary'], 'Number · on rows')
        self.assertEqual(work[1]['summary'], 'Text · archived')
        for scope in ('@inbox', '@all', ''):
            defs = self.query('field_defs', scope)
            self.assertEqual([r['name'] for r in defs], ['Stage'], scope)
        self.assertEqual(defs[0]['summary'], 'Select · 3 options')
        self.assertEqual(self.query('field_detail', 'sel')[0]['name'], 'Stage')
        self.assertEqual(self.query('field_scope', '@inbox')[0]['list_id'], '')
        self.assertEqual(self.query('field_scope', 'work')[0]['list_id'], 'work')
        self.assertEqual([(r['value'], r['label']) for r in self.query('field_scope_choices', 'work')],
                         [('work', 'This list (Work)'), ('', 'All lists')])
        self.assertEqual([(r['value'], r['label']) for r in self.query('field_scope_choices', '')], [('', 'All lists')])

    # ---- export -----------------------------------------------------------

    def test_csv_export_has_a_custom_fields_column(self):
        self.field('n', 'Budget', 'number', position=0)
        self.field('s', 'Stage', 'select', options=STAGE, position=1)
        self.field('old', 'Legacy', 'text', archived=1)
        self.task('with')
        self.task('without')
        self.value('with', 'n', '1200')
        self.value('with', 's', 'x')
        self.value('with', 'old', 'x')
        csv = next(e for e in MANIFEST['export'] if e['format'] == 'csv')
        self.assertEqual(csv['columns'][-3:], ['custom_fields', 'estimate', 'tracked'])
        rows = {r['title']: r for r in self.query('export_tasks')}
        self.assertEqual(rows['with']['custom_fields'], 'Budget: 1200 · Stage: Done')
        self.assertIsNone(rows['without']['custom_fields'])
        for column in csv['columns']:
            self.assertIn(column, rows['with'])

    def test_workspace_query_declares_the_filter_for_saved_views(self):
        query = MANIFEST['queries']['workspace_tasks']
        # include_archived (FIX-19) and nesting (FIX-21) are appended after it.
        self.assertEqual(query['params'][-3:], ['field_values', 'include_archived', 'nesting'])
        self.assertEqual(query['view']['params']['field_values'], '')
        flt = next(f for f in query['view']['filters'] if f['param'] == 'field_values')
        self.assertEqual((flt['multiple'], flt['choices_query']), (True, 'field_filter_choices'))
        self.assertIn('field_filter_choices', MANIFEST['queries'])


if __name__ == '__main__':
    import unittest
    unittest.main()

"""Read-model behavior. Real host migration is covered in runtime integration tests."""
import json
from pathlib import Path
import sqlite3
import unittest

MANIFEST = json.loads((Path(__file__).resolve().parents[1] / 'manifest.json').read_text())


class OrganizationQuery(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.row_factory = sqlite3.Row
        for table in MANIFEST['storage']['tables']:
            columns = ['id TEXT PRIMARY KEY', '_deleted INTEGER']
            for col in table['columns']:
                kind = 'TEXT' if col['type'] == 'text' else 'INTEGER'
                default = ''
                if 'default' in col:
                    default = ' DEFAULT ' + str(int(col['default']))
                columns.append(f'"{col["name"]}" {kind}{default}')
            self.db.execute(f'CREATE TABLE "{table["name"]}" ({",".join(columns)})')

    def tearDown(self):
        self.db.close()

    def insert(self, table, **values):
        keys = ','.join(values)
        slots = ','.join('?' for _ in values)
        self.db.execute(f'INSERT INTO {table} ({keys}) VALUES ({slots})', list(values.values()))

    def query(self, name='organization_tasks', *params):
        query = MANIFEST['queries'][name]
        if not params:
            params = [None] * len(query['params'])
        elif len(params) < len(query['params']):
            params = list(params) + [None] * (len(query['params']) - len(params))
        return [dict(row) for row in self.db.execute(query['sql'], params)]

    def test_legacy_tasks_keep_completion_and_default_organization(self):
        for id, done, progress in [('a', 0, 0), ('b', 0, 1), ('c', 1, 1)]:
            self.insert('tasks', id=id, title=id, done=done, in_progress=progress)
        before = [tuple(r) for r in self.db.execute('SELECT * FROM tasks')]
        rows = self.query()
        self.assertEqual([r['status_name'] for r in rows], ['To pick', 'In progress', 'Done'])
        self.assertEqual([r['status_category'] for r in rows], ['not_started', 'active', 'completed'])
        for row in rows:
            self.assertEqual((row['list_name'], row['group_name']), ('Inbox', 'Ungrouped'))
            self.assertIsNone(row['planned_for'])
            self.assertEqual(row['sort_order'], 0)
            self.assertEqual((row['invalid_list'], row['invalid_group'], row['invalid_status']), (0, 0, 0))
        self.assertEqual(before, [tuple(r) for r in self.db.execute('SELECT * FROM tasks')])

    def test_custom_labels_and_planning_are_independent_of_deadline(self):
        self.insert('task_lists', id='l', name='Work')
        self.insert('task_groups', id='g', list_id='l', name='Launch')
        self.insert('task_statuses', id='s', list_id='l', name='Reviewing', category='active')
        self.insert('tasks', id='t', title='Ship', list_id='l', group_id='g', status_id='s',
                    in_progress=1, due='2026-12-01', planned_for='2026-11-12')
        row = self.query()[0]
        self.assertEqual((row['list_name'], row['group_name'], row['status_name']), ('Work', 'Launch', 'Reviewing'))
        self.assertEqual(row['planned_for'], '2026-11-12')
        self.assertEqual(self.db.execute('SELECT due FROM tasks').fetchone()[0], '2026-12-01')

    def test_invalid_references_are_preserved_and_flagged(self):
        self.insert('task_groups', id='g', list_id='other', name='Wrong group')
        self.insert('task_statuses', id='s', list_id='other', name='Wrong status', category='active')
        self.insert('tasks', id='t', title='Lost refs', list_id='missing', group_id='g', status_id='s', in_progress=1)
        row = self.query()[0]
        self.assertEqual((row['list_id'], row['group_id'], row['status_id']), ('missing', 'g', 's'))
        self.assertEqual((row['invalid_list'], row['invalid_group'], row['invalid_status']), (1, 1, 1))
        self.assertEqual((row['list_name'], row['group_name'], row['status_name']), ('Inbox', 'Ungrouped', 'In progress'))

    def test_old_completion_controls_cannot_display_contradictory_custom_status(self):
        self.insert('task_statuses', id='s', name='Reviewing', category='active')
        self.insert('tasks', id='t', title='Done', status_id='s', done=1)
        row = self.query()[0]
        self.assertEqual(row['status_name'], 'Done')
        self.assertEqual(row['status_id'], 's')
        self.assertEqual(row['invalid_status'], 1)

    def test_archived_definitions_keep_labels_deleted_definitions_are_flagged(self):
        self.insert('task_lists', id='l', name='Old project', archived=1)
        self.insert('task_groups', id='g', list_id='l', name='Old group', _deleted=123)
        self.insert('tasks', id='t', title='History', list_id='l', group_id='g')
        row = self.query()[0]
        self.assertEqual(row['list_name'], 'Old project')
        self.assertEqual(row['invalid_list'], 0)
        self.assertEqual(row['invalid_group'], 1)

    def test_scope_filters_and_hidden_tasks(self):
        for id, list_id in [('a', None), ('b', 'work')]:
            self.insert('task_groups', id=id, list_id=list_id, name=id)
            self.insert('task_statuses', id=id, list_id=list_id, name=id, category='not_started')
        for query in ['task_groups', 'task_statuses']:
            self.assertEqual([r['id'] for r in self.query(query, '')], ['a'])
            self.assertEqual([r['id'] for r in self.query(query, 'work')], ['b'])
        self.insert('tasks', id='visible', title='Visible')
        self.insert('tasks', id='trash', title='Trash', _deleted=1)
        self.insert('tasks', id='cleared', title='Cleared', done=1, cleared_at=1)
        self.assertEqual([r['id'] for r in self.query()], ['visible'])
        self.assertEqual(self.query('organization_tasks', 'trash'), [])


if __name__ == '__main__':
    unittest.main()

"""Behavior proof for the Notes sidebar: counts equal list rows per view.

Run: python3 -m unittest discover -s packages/notes/tests -p '*_query.py'
"""
import json
from pathlib import Path
import sqlite3
import unittest

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = json.loads((ROOT / 'manifest.json').read_text())
Q = MANIFEST['queries']
LIST = Q['list']['sql']
BEFORE = (ROOT / 'tests/fixtures/list-before.sql').read_text()
DAY = 86400000
TODAY = '2026-11-15'
NOW = 1794700800000 + 12 * 3600000  # 2026-11-15 12:00 UTC


class NavQueries(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.row_factory = sqlite3.Row
        columns = ['id TEXT PRIMARY KEY', '_deleted INTEGER']
        for column in MANIFEST['storage']['tables'][0]['columns']:
            kind = 'TEXT' if column['type'] == 'text' else 'INTEGER'
            columns.append(f'"{column["name"]}" {kind}')
        self.db.execute('CREATE TABLE notes (' + ','.join(columns) + ')')
        rows = [
            # id, title, body, project, pinned, updated_days_ago, deleted
            ('a', 'Plain', 'text', None, 0, 0, None),
            ('b', 'Old', 'text', '', 0, 30, None),
            ('c', 'Pinned old', 'text', 'Work', 1, 40, None),
            ('d', 'Pinned new', 'text', 'Work', 1, 1, None),
            ('e', 'Work new', 'text', 'Work', 0, 2, None),
            ('f', 'Home', 'text', 'Home', 0, 9, None),
            ('g', 'Task note', 'text', 'Tasks', 0, 1, None),
            ('h', 'Pinned task', 'text', 'Tasks', 1, 1, None),
            ('i', 'Gone', 'text', 'Work', 0, 1, 1),
            ('j', 'Gone task', 'text', 'Tasks', 0, 1, 1),
            ('k', 'Untitled', '', None, 0, 1, 1),
            ('l', 'Edge 6 days', 'text', None, 0, 6, None),
            ('m', 'Edge 7 days', 'text', None, 0, 7, None),
            ('n', 'No time', 'text', None, 0, None, None),
        ]
        for id_, title, body, project, pinned, ago, deleted in rows:
            stamp = None if ago is None else NOW - ago * DAY
            self.db.execute('INSERT INTO notes (id,title,body,project,pinned,created_at,updated_at,_deleted) VALUES (?,?,?,?,?,?,?,?)',
                            (id_, title, body, project, pinned, stamp, stamp, deleted))

    def tearDown(self):
        self.db.close()

    def rows(self, view, project='', q=''):
        return [dict(r) for r in self.db.execute(LIST, [q, project, 0, TODAY, view])]

    def ids(self, view, project=''):
        return sorted(r['id'] for r in self.rows(view, project))

    def counts(self):
        return dict(self.db.execute(Q['nav_counts']['sql'], [0, TODAY]).fetchone())

    def test_view_filters(self):
        self.assertEqual(self.ids(''), sorted('abcdefl mn'.replace(' ', '')))
        self.assertEqual(self.ids('all'), self.ids(''))
        self.assertEqual(self.ids('pinned'), ['c', 'd'])
        self.assertEqual(self.ids('recent'), ['a', 'd', 'e', 'l'])
        self.assertEqual(self.ids('tasks'), ['g', 'h'])
        self.assertEqual(self.ids('trash'), ['i', 'j'])
        self.assertEqual(self.ids('project', 'Work'), ['c', 'd', 'e'])

    def test_counts_equal_rows_for_every_view(self):
        counts = self.counts()
        for view, key in [('all', 'all_count'), ('pinned', 'pinned'), ('recent', 'recent'),
                          ('tasks', 'tasks'), ('trash', 'trash')]:
            with self.subTest(view=view):
                self.assertEqual(counts[key], len(self.rows(view)))

    def test_project_counts_equal_project_rows(self):
        projects = [dict(r) for r in self.db.execute(Q['nav_projects']['sql'], ['Work'])]
        self.assertEqual([p['project'] for p in projects], ['Home', 'Work'])
        for p in projects:
            with self.subTest(project=p['project']):
                self.assertEqual(p['count'], len(self.rows('project', p['project'])))
        self.assertEqual([bool(p['selected']) for p in projects], [False, True])

    def test_sections_and_order(self):
        self.assertEqual({r['section'] for r in self.rows('trash')}, {None})
        self.assertEqual({r['section'] for r in self.rows('pinned')}, {None})
        recent = self.rows('recent')
        self.assertEqual(recent[0]['id'], 'a')  # newest first, pinned not floated
        self.assertEqual({r['section'] for r in self.rows('')}, {'Pinned', 'Notes'})

    def test_empty_view_matches_old_output(self):
        for params in (['', '', 0, TODAY], ['Work', '', 3600000, TODAY], ['', 'Tasks', 0, TODAY], ['', 'Home', 0, TODAY]):
            for view in ('', None):
                with self.subTest(params=params, view=view):
                    old = self.db.execute(BEFORE, params).fetchall()
                    new = self.db.execute(LIST, params + [view]).fetchall()
                    self.assertEqual(old, new)

    def test_list_declares_view_param_last(self):
        self.assertEqual(Q['list']['params'], ['q', 'project', 'utc_offset_ms', 'today', 'view'])


if __name__ == '__main__':
    unittest.main()

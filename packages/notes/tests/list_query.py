"""Behavior proof for limiting candidates before computing note presentation.

Run: python3 -m unittest discover -s packages/notes/tests -p '*_query.py'
The frozen pre-optimization SQL is a reference for the existing output contract.
"""
import json
from pathlib import Path
import sqlite3
import unittest

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = json.loads((ROOT / 'manifest.json').read_text())
CURRENT = MANIFEST['queries']['list']['sql']
BEFORE = (ROOT / 'tests/fixtures/list-before.sql').read_text()

class NoteListQuery(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        columns = ['id TEXT PRIMARY KEY', '_deleted INTEGER']
        for column in MANIFEST['storage']['tables'][0]['columns']:
            kind = 'TEXT' if column['type'] == 'text' else 'INTEGER'
            columns.append(f'"{column["name"]}" {kind}')
        self.db.execute('CREATE TABLE notes (' + ','.join(columns) + ')')
        for i in range(1200):
            self.db.execute('INSERT INTO notes (id,title,body,project,pinned,created_at,updated_at,_deleted) VALUES (?,?,?,?,?,?,?,?)', (
                f'{i:05d}', f'Note {i}' + (' needle' if i % 11 == 0 else ''),
                '# Heading\n- [x] Done\n- [ ] Todo\n' + ('body-match' if i % 13 == 0 else '**text**'),
                [None, '', 'Work', 'Tasks', 'Personal'][i % 5], int(i % 17 == 0),
                1793745000000 if i % 7 else None, 1793745000000 + (i % 31) * 1000 if i % 3 else None,
                1793745000000 if i % 29 == 0 else None,
            ))

    def tearDown(self):
        self.db.close()

    def test_output_matches_existing_contract_across_filters_dates_and_ties(self):
        for text in [None, '', 'needle', 'body-match', 'not-found', 'Work']:
            for project in [None, '', 'Work', 'Tasks', 'Personal', 'missing']:
                for offset, day in [(0, '2026-11-04'), (19800000, '2026-11-05'), (-28800000, '2027-01-01')]:
                    with self.subTest(text=text, project=project, offset=offset, day=day):
                        params = [text, project, offset, day]
                        old = self.db.execute(BEFORE, params).fetchall()
                        new = self.db.execute(CURRENT, params).fetchall()
                        self.assertEqual(old, new)
                        self.assertLessEqual(len(new), 300)

    def test_empty_and_unpinned_lists_match(self):
        for operation in ['UPDATE notes SET pinned=0', 'DELETE FROM notes']:
            self.db.execute(operation)
            params = ['', '', 0, '2026-11-04']
            self.assertEqual(self.db.execute(BEFORE, params).fetchall(), self.db.execute(CURRENT, params).fetchall())

if __name__ == '__main__':
    unittest.main()

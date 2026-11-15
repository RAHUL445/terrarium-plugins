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
        self.db.row_factory = sqlite3.Row
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
                        new = self.db.execute(CURRENT, params + ['']).fetchall()
                        self.assertEqual(old, new)
                        self.assertLessEqual(len(new), 300)

    def test_empty_and_unpinned_lists_match(self):
        for operation in ['UPDATE notes SET pinned=0', 'DELETE FROM notes']:
            self.db.execute(operation)
            params = ['', '', 0, '2026-11-04']
            self.assertEqual(self.db.execute(BEFORE, params).fetchall(), self.db.execute(CURRENT, params + ['']).fetchall())

    def review_notes(self, start=None, end=None, page_size=100, offset=0,
                     sort='updated_at', direction='desc'):
        query = MANIFEST['queries']['review_notes']
        return [dict(row) for row in self.db.execute(query['sql'],
                [start, end, page_size, offset, sort, direction])]

    def test_review_notes_use_created_at_fallback_half_open_range_and_stable_ties(self):
        for note_id, created_at, updated_at in [
            ('before', 999, None), ('start', 1000, None),
            ('tie-b', 900, 1500), ('tie-a', 900, 1500),
            ('end', 900, 2000), ('missing', None, None),
        ]:
            self.db.execute('INSERT INTO notes (id,title,created_at,updated_at,_deleted) VALUES (?,?,?,?,NULL)',
                            (note_id, note_id, created_at, updated_at))
        rows = self.review_notes(1000, 2000, 100, 0, 'updated_at', 'asc')
        self.assertEqual([row['id'] for row in rows], ['start', 'tie-a', 'tie-b'])
        self.assertEqual([row['updated_at'] for row in rows], [1000, 1500, 1500])
        self.assertEqual(MANIFEST['queries']['review_notes']['view']['title'], 'Created or edited notes')

    def test_review_notes_pagination_reaches_rows_beyond_legacy_300_limit(self):
        for index in range(425):
            note_id = f'review-{index:04d}'
            self.db.execute('INSERT INTO notes (id,title,created_at,updated_at,_deleted) VALUES (?,?,900,1500,NULL)',
                            (note_id, f'Note {index:04d}'))
        page = self.review_notes(1000, 2000, 101, 300, 'updated_at', 'asc')
        tail = self.review_notes(1000, 2000, 101, 400, 'updated_at', 'asc')
        self.assertEqual(len(page), 101)
        self.assertEqual(page[0]['id'], 'review-0300')
        self.assertEqual(page[-1]['id'], 'review-0400')
        self.assertEqual([row['id'] for row in tail], [f'review-{i:04d}' for i in range(400, 425)])

if __name__ == '__main__':
    unittest.main()

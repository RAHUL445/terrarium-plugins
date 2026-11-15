"""Behavior proof for the Notes editor reads: link-free snippets and the `note` query.

Run: python3 -m unittest discover -s packages/notes/tests -p '*_query.py'
"""
import json
from pathlib import Path
import sqlite3
import unittest

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = json.loads((ROOT / 'manifest.json').read_text())
Q = MANIFEST['queries']
NOW = 1794700800000 + 12 * 3600000  # 2026-11-15 12:00 UTC
TODAY = '2026-11-15'


class EditorQueries(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.row_factory = sqlite3.Row
        columns = ['id TEXT PRIMARY KEY', '_deleted INTEGER']
        for column in MANIFEST['storage']['tables'][0]['columns']:
            kind = 'TEXT' if column['type'] == 'text' else 'INTEGER'
            columns.append(f'"{column["name"]}" {kind}')
        self.db.execute('CREATE TABLE notes (' + ','.join(columns) + ')')

    def tearDown(self):
        self.db.close()

    def add(self, id_, title, body, ago_ms=0):
        self.db.execute('INSERT INTO notes (id,title,body,created_at,updated_at) VALUES (?,?,?,?,?)',
                        (id_, title, body, NOW - ago_ms, NOW - ago_ms))

    def snippet(self, body):
        self.add('x', 'T', 'First line\n' + body)
        row = self.db.execute(Q['list']['sql'], ['', '', 0, TODAY, '']).fetchone()
        self.db.execute('DELETE FROM notes')
        return row['snippet']

    def test_snippet_keeps_link_and_image_text_only(self):
        self.assertEqual(self.snippet('See [the plan](https://x.test/a?b=1) now'), 'See the plan now')
        self.assertEqual(self.snippet('![diagram](terrarium:blob/abc) after'), 'diagram after')
        self.assertEqual(self.snippet('[a](u1) and [b](terrarium:todo/tasks/t1) and ![c](u3)'), 'a and b and c')
        self.assertEqual(self.snippet('- [ ] todo\n- [x] done'), '☐ todo ☑ done')
        self.assertEqual(self.snippet('plain **bold** text'), 'plain bold text')
        self.assertEqual(self.snippet('broken [link](no close'), 'broken link(no close')

    def test_snippet_is_cut_to_160_characters(self):
        self.assertLessEqual(len(self.snippet('[w](u) ' * 100)), 160)

    def note(self, id_):
        row = self.db.execute(Q['note']['sql'], [id_, 0, TODAY]).fetchone()
        return dict(row) if row else None

    def test_note_title_is_blank_when_it_follows_the_first_line(self):
        self.add('a', 'Plan', '# Plan\nbody')
        self.add('b', 'Own title', '# Plan\nbody')
        self.add('c', 'Untitled', '')
        self.assertEqual(self.note('a')['title'], '')
        self.assertEqual(self.note('b')['title'], 'Own title')
        self.assertEqual(self.note('c')['title'], '')
        self.assertEqual(self.note('c')['name'], 'Untitled')
        self.assertEqual(self.note('b')['name'], 'Own title')

    def test_note_keeps_old_columns_and_adds_edited(self):
        self.add('a', 'T', 'body', 2 * 86400000)
        row = self.note('a')
        for key in ('id', 'title', 'body', 'project', 'pinned'):
            self.assertIn(key, row)
        self.assertEqual(row['edited'], 'Fri')
        self.add('b', 'T', 'body', 0)
        self.assertEqual(self.note('b')['edited'], '12:00')

    def test_note_edited_is_empty_without_host_values(self):
        self.add('a', 'T', 'body')
        row = self.db.execute(Q['note']['sql'], ['a', None, None]).fetchone()
        self.assertIsNone(row['edited'])

    def test_note_params_start_with_id(self):
        self.assertEqual(Q['note']['params'], ['id', 'utc_offset_ms', 'today'])


if __name__ == '__main__':
    unittest.main()

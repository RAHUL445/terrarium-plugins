"""SQL contract tests for the paged Notes host-view query."""
import json
from pathlib import Path
import sqlite3
import unittest

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = json.loads((ROOT / 'manifest.json').read_text())
QUERY = MANIFEST['queries']['host_notes']

class NotesHostViewQuery(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.execute('CREATE TABLE notes (id TEXT PRIMARY KEY, title TEXT, body TEXT, project TEXT, pinned INTEGER, created_at INTEGER, updated_at INTEGER, _deleted INTEGER)')
        self.db.executemany(
            'INSERT INTO notes VALUES (?, ?, ?, ?, ?, ?, ?, NULL)',
            [(f'note-{i:04d}', f'Note {i:04d}', 'body needle' if i % 10 == 0 else 'body', 'Work', 0, i * 1000, i * 1000, ) for i in range(1350)],
        )

    def tearDown(self):
        self.db.close()

    def run_page(self, *, q='', project='', sort='updated_at', direction='desc', size=100, offset=0):
        return self.db.execute(QUERY['sql'], [q, project, sort, direction, size, offset]).fetchall()

    def test_pagination_reaches_past_legacy_list_cap_with_stable_order(self):
        rows = self.run_page(offset=300)
        self.assertEqual(len(rows), 100)
        self.assertEqual(rows[0]['id'] if isinstance(rows[0], sqlite3.Row) else rows[0][0], 'note-1049')
        self.assertEqual(rows[-1][0], 'note-0950')

    def test_filters_and_sorting_are_applied_before_pagination(self):
        rows = self.run_page(q='needle', sort='title', direction='asc', size=20)
        self.assertEqual(len(rows), 20)
        self.assertEqual(rows[0][0], 'note-0000')
        self.assertEqual(rows[-1][0], 'note-0190')
        tasks = self.db.execute("UPDATE notes SET project='Tasks' WHERE id='note-0000'")
        rows = self.run_page(q='needle', project='')
        self.assertNotIn('note-0000', [row[0] for row in rows])
        self.assertEqual(tasks.rowcount, 1)

    def test_widget_uses_named_field_presets_and_individual_filters(self):
        widget = next(w for w in MANIFEST['widgets'] if w['id'] == 'host-view')
        template = json.loads((ROOT / widget['template']).read_text())
        config = template['body']['config']
        self.assertIsInstance(config['filters'], dict)
        self.assertEqual(config['fields'], '{{$config.fields}}')
        page_config = template['page']['config']
        self.assertEqual(page_config['fields']['type'], 'enum')
        self.assertNotIn('filters', page_config)
        for key in ['q', 'project']:
            self.assertIn(key, config['filters'])
            self.assertIn(key, page_config)

    def test_manifest_declares_v1_paging_and_numeric_date_column(self):
        self.assertEqual(QUERY['view']['version'], 1)
        self.assertEqual(QUERY['view']['pagination']['max_page_size'], 1000)
        self.assertEqual(next(c for c in QUERY['view']['columns'] if c['key'] == 'updated_at')['type'], 'date')
        self.assertIn('data-views-v1', MANIFEST['requires'])

if __name__ == '__main__':
    unittest.main()

"""SQL contract tests for the paged Bookmarks host-view query."""
import json
from pathlib import Path
import sqlite3
import unittest

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = json.loads((ROOT / 'manifest.json').read_text())
QUERY = MANIFEST['queries']['host_bookmarks']

class BookmarksHostViewQuery(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.execute('CREATE TABLE bookmarks (id TEXT PRIMARY KEY, url TEXT, title TEXT, project TEXT, tags TEXT, note TEXT, visits INTEGER, visited_at INTEGER, pinned INTEGER, created_at INTEGER, _deleted INTEGER)')
        self.db.executemany(
            'INSERT INTO bookmarks VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)',
            [(f'bookmark-{i:04d}', f'https://example{i}.test/page', f'Bookmark {i:04d}', 'Reading', 'work, saved', 'note', i % 7, i * 100, 0, i * 1000) for i in range(750)],
        )

    def tearDown(self):
        self.db.close()

    def run_page(self, *, q='', project='', tag='', sort='new', size=100, offset=0):
        return self.db.execute(QUERY['sql'], [q, project, tag, sort, size, offset]).fetchall()

    def test_pagination_reaches_past_legacy_list_cap(self):
        rows = self.run_page(offset=500)
        self.assertEqual(len(rows), 100)
        self.assertEqual(rows[0][0], 'bookmark-0249')
        self.assertEqual(rows[-1][0], 'bookmark-0150')

    def test_search_and_tag_filters_precede_pagination(self):
        rows = self.run_page(q='Bookmark 07', tag='saved', size=30)
        self.assertEqual(len(rows), 30)
        self.assertTrue(all('Bookmark 07' in row[1] and row[5] == '#work #saved' for row in rows))
        missing = self.run_page(project='missing')
        self.assertEqual(missing, [])

    def test_widget_uses_named_field_presets_and_individual_filters(self):
        widget = next(w for w in MANIFEST['widgets'] if w['id'] == 'host-view')
        template = json.loads((ROOT / widget['template']).read_text())
        config = template['body']['config']
        self.assertIsInstance(config['filters'], dict)
        self.assertEqual(config['fields'], '{{$config.fields}}')
        page_config = template['page']['config']
        self.assertEqual(page_config['fields']['type'], 'enum')
        self.assertNotIn('filters', page_config)
        for key in ['q', 'project', 'tag']:
            self.assertIn(key, config['filters'])
            self.assertIn(key, page_config)

    def test_manifest_declares_capability_and_numeric_date_column(self):
        self.assertIn('data-views-v1', MANIFEST['requires'])
        self.assertEqual(QUERY['view']['version'], 1)
        self.assertEqual(next(c for c in QUERY['view']['columns'] if c['key'] == 'created_at')['type'], 'date')
        self.assertEqual(QUERY['view']['pagination']['max_page_size'], 1000)

if __name__ == '__main__':
    unittest.main()

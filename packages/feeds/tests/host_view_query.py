"""SQL contract tests for the paged Feeds host-view query."""
import json
from pathlib import Path
import sqlite3
import unittest

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = json.loads((ROOT / 'manifest.json').read_text())
QUERY = MANIFEST['queries']['host_unread']

class FeedsHostViewQuery(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.execute('CREATE TABLE feeds (id TEXT PRIMARY KEY, title TEXT, _deleted INTEGER)')
        self.db.execute('CREATE TABLE items (id TEXT PRIMARY KEY, feed_id TEXT, title TEXT, summary TEXT, link TEXT, published_ms INTEGER, read INTEGER, _deleted INTEGER)')
        self.db.execute("INSERT INTO feeds VALUES ('feed-1', 'Example feed', NULL)")
        self.db.executemany(
            'INSERT INTO items VALUES (?, ?, ?, ?, ?, ?, ?, NULL)',
            [(f'item-{i:03d}', 'feed-1', f'Article {i:03d}', 'summary', f'https://example.test/{i}', i * 1000, 0) for i in range(37)]
            + [(f'read-{i:03d}', 'feed-1', f'Read {i:03d}', 'summary', f'https://example.test/read/{i}', i * 1000, 1) for i in range(8)],
        )

    def tearDown(self):
        self.db.close()

    def test_pagination_reaches_beyond_legacy_unread_widget_limit(self):
        rows = self.db.execute(QUERY['sql'], [True, 10, 10]).fetchall()
        self.assertEqual(len(rows), 10)
        self.assertEqual(rows[0][0], 'item-026')
        self.assertEqual(rows[-1][0], 'item-017')
        self.assertTrue(all(row[0].startswith('item-') for row in rows))

    def test_unread_filter_can_expand_to_all_items(self):
        rows = self.db.execute(QUERY['sql'], [False, 100, 0]).fetchall()
        self.assertEqual(len(rows), 45)
        self.assertTrue(any(row[0].startswith('read-') for row in rows))

    def test_row_action_preserves_mark_read_and_widget_is_opt_in(self):
        action = QUERY['view']['row_action']
        self.assertEqual(action['action']['op'], 'mark_read')
        self.assertEqual(action['action']['args'], {'id': '{{row.id}}', 'read': True})
        widget = next(w for w in MANIFEST['widgets'] if w['id'] == 'host-view')
        template = json.loads((ROOT / widget['template']).read_text())
        self.assertEqual(template['home'], {'hidden': True})
        self.assertEqual(template['rail']['default'], False)
        self.assertEqual(template['body']['type'], 'data-view')

    def test_widget_uses_named_field_presets_and_individual_filters(self):
        widget = next(w for w in MANIFEST['widgets'] if w['id'] == 'host-view')
        template = json.loads((ROOT / widget['template']).read_text())
        config = template['body']['config']
        self.assertIsInstance(config['filters'], dict)
        self.assertEqual(config['fields'], '{{$config.fields}}')
        page_config = template['page']['config']
        self.assertEqual(page_config['fields']['type'], 'enum')
        self.assertNotIn('filters', page_config)
        for key in ['unread']:
            self.assertIn(key, config['filters'])
            self.assertIn(key, page_config)

    def test_manifest_declares_v1_paging_and_numeric_date_column(self):
        self.assertIn('data-views-v1', MANIFEST['requires'])
        self.assertEqual(QUERY['view']['version'], 1)
        self.assertEqual(QUERY['view']['params']['unread'], True)
        self.assertEqual(next(c for c in QUERY['view']['columns'] if c['key'] == 'published_ms')['type'], 'date')
        self.assertEqual(QUERY['view']['pagination']['max_page_size'], 1000)
        self.assertEqual(QUERY['view']['filters'][0]['type'], 'boolean')

if __name__ == '__main__':
    unittest.main()

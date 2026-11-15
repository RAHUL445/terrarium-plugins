"""Today view (today-v1) source for Feeds: the newest unread items."""
import json
from pathlib import Path
import sqlite3
import unittest

MANIFEST = json.loads((Path(__file__).resolve().parents[1] / 'manifest.json').read_text())
NOW = 1794441600000
HOUR = 3600000


class FeedsTodayQuery(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.row_factory = sqlite3.Row
        self.db.execute('CREATE TABLE feeds (id TEXT PRIMARY KEY, title TEXT, _deleted INTEGER)')
        self.db.execute('CREATE TABLE items (id TEXT PRIMARY KEY, feed_id TEXT, title TEXT, link TEXT, published_ms INTEGER, read INTEGER, _deleted INTEGER)')
        self.db.execute("INSERT INTO feeds VALUES ('rust', 'Rust Blog', NULL), ('gone', 'Removed', 1)")

    def tearDown(self):
        self.db.close()

    def add(self, id, published_ms, read=0, feed='rust', deleted=None):
        self.db.execute('INSERT INTO items VALUES (?, ?, ?, ?, ?, ?, ?)',
                        (id, feed, id.title(), f'https://blog.rust-lang.org/{id}', published_ms, read, deleted))

    def test_top_three_unread_newest_first_with_age(self):
        self.add('a', NOW - 26 * HOUR)
        self.add('b', NOW - 2 * HOUR)
        self.add('c', NOW - 10 * 24 * HOUR)
        self.add('d', NOW - 20 * 24 * HOUR)
        self.add('read', NOW - HOUR, read=1)
        self.add('trashed', NOW - HOUR, deleted=1)
        self.add('orphan', NOW - HOUR, feed='gone')
        rows = [dict(r) for r in self.db.execute(MANIFEST['queries']['today_unread']['sql'], [NOW])]
        self.assertEqual([r['id'] for r in rows], ['b', 'a', 'c'])
        self.assertEqual([r['source_label'] for r in rows], ['Rust Blog · 2h', 'Rust Blog · 1d', 'Rust Blog · 10d'])
        count = dict(self.db.execute(MANIFEST['queries']['unread_count']['sql']).fetchone())
        self.assertEqual(count, {'noun': 'unread', 'count': 4})

    def test_open_marks_read_and_uses_the_article_link(self):
        source = MANIFEST['today'][0]
        self.assertEqual((source['section'], source['count_query'], source['limit']), ('catch_up', 'unread_count', 3))
        self.assertEqual(source['row']['open'], {'url': '{{row.link}}', 'op': 'mark_read', 'args': {'id': '{{row.id}}', 'read': True}})


if __name__ == '__main__':
    unittest.main()

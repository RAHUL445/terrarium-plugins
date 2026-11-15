"""Today view (today-v1) source for Bookmarks: recent links without tags."""
import json
from pathlib import Path
import sqlite3
import unittest

MANIFEST = json.loads((Path(__file__).resolve().parents[1] / 'manifest.json').read_text())
NOW = 1794441600000
DAY = 86400000


class BookmarksTodayQuery(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.row_factory = sqlite3.Row
        self.db.execute('CREATE TABLE bookmarks (id TEXT PRIMARY KEY, _deleted INTEGER, url TEXT, title TEXT, tags TEXT, created_at INTEGER)')

    def tearDown(self):
        self.db.close()

    def add(self, id, created_at, tags=None, title=None, deleted=None, url=None):
        self.db.execute('INSERT INTO bookmarks VALUES (?, ?, ?, ?, ?, ?)',
                        (id, deleted, url or f'https://doc.rust-lang.org/{id}', title, tags, created_at))

    def query(self, name):
        return [dict(row) for row in self.db.execute(MANIFEST['queries'][name]['sql'], [NOW])]

    def test_untagged_links_from_the_last_seven_days(self):
        self.add('fresh', NOW - DAY, title='Nomicon')
        self.add('blank-tags', NOW - 2 * DAY, tags='  ')
        self.add('tagged', NOW - DAY, tags='rust')
        self.add('old', NOW - 8 * DAY)
        self.add('trashed', NOW - DAY, deleted=1)
        rows = self.query('today_untagged')
        self.assertEqual([r['id'] for r in rows], ['fresh', 'blank-tags'])
        self.assertEqual(rows[0]['title'], 'Nomicon')
        self.assertEqual(rows[1]['title'], 'https://doc.rust-lang.org/blank-tags')
        self.assertEqual(rows[0]['host'], 'doc.rust-lang.org')
        self.assertEqual(self.query('today_untagged_count'), [{'count': 2}])

    def test_source_opens_the_editor(self):
        source = MANIFEST['today'][0]
        self.assertEqual(source['section'], 'sort')
        self.assertEqual(source['row']['open'], {'navigate': {'item': '{{row.id}}'}})


if __name__ == '__main__':
    unittest.main()

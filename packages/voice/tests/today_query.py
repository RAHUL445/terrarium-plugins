"""Today view (today-v1) source for Voice: captures waiting for review."""
import json
from pathlib import Path
import sqlite3
import unittest

MANIFEST = json.loads((Path(__file__).resolve().parents[1] / 'manifest.json').read_text())


class VoiceTodayQuery(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.row_factory = sqlite3.Row
        self.db.execute('CREATE TABLE captures (id TEXT PRIMARY KEY, _deleted INTEGER, text TEXT, source TEXT, status TEXT, created_at INTEGER)')

    def tearDown(self):
        self.db.close()

    def add(self, id, text, status='ready', created_at=0, deleted=None, source='text'):
        self.db.execute('INSERT INTO captures VALUES (?, ?, ?, ?, ?, ?)', (id, deleted, text, source, status, created_at))

    def query(self, name):
        return [dict(row) for row in self.db.execute(MANIFEST['queries'][name]['sql'])]

    def test_review_lists_ready_captures_newest_first_with_first_line_title(self):
        self.add('old', 'Idea: compost bin\nbehind the shed', created_at=1)
        self.add('new', 'Call the plumber', created_at=2, source='audio')
        self.add('queued', 'Still transcribing', status='queued', created_at=3)
        self.add('sent', 'Done already', status='sent', created_at=4)
        self.add('trashed', 'Gone', created_at=5, deleted=1)
        rows = self.query('today_review')
        self.assertEqual([(r['id'], r['title'], r['is_audio']) for r in rows],
                         [('new', 'Call the plumber', 1), ('old', 'Idea: compost bin', 0)])
        self.assertEqual(self.query('badge'), [{'count': 2}])

    def test_long_first_lines_are_shortened(self):
        self.add('long', 'x' * 120)
        self.assertEqual(self.query('today_review')[0]['title'], 'x' * 88 + '…')

    def test_actions_reuse_send_and_archive(self):
        source = MANIFEST['today'][0]
        self.assertEqual((source['section'], source['count_query']), ('sort', 'badge'))
        ops = [(a['action']['op'], a['action']['args'].get('to')) for a in source['row']['actions']]
        self.assertEqual(ops, [('send', 'todo'), ('send', 'notes'), ('archive', None)])
        self.assertEqual(source['row']['actions'][2]['action']['undo']['op'], 'reopen')


if __name__ == '__main__':
    unittest.main()

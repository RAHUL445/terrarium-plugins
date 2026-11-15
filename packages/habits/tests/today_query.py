"""Today view (today-v1) sources for Habits."""
import json
from pathlib import Path
import sqlite3
import unittest

MANIFEST = json.loads((Path(__file__).resolve().parents[1] / 'manifest.json').read_text())
TODAY = '2026-11-12'  # a Thursday: weekday 3


class HabitsTodayQuery(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(':memory:')
        self.db.row_factory = sqlite3.Row
        for table in MANIFEST['storage']['tables']:
            columns = ['id TEXT PRIMARY KEY', '_deleted INTEGER']
            for col in table['columns']:
                kind = 'TEXT' if col['type'] == 'text' else 'INTEGER'
                default = f" DEFAULT {int(col['default'])}" if 'default' in col else ''
                columns.append(f'"{col["name"]}" {kind}{default}')
            self.db.execute(f'CREATE TABLE "{table["name"]}" ({",".join(columns)})')

    def tearDown(self):
        self.db.close()

    def insert(self, table, **values):
        self.db.execute(f'INSERT INTO {table} ({",".join(values)}) VALUES ({",".join("?" for _ in values)})',
                        list(values.values()))

    def query(self, name, *params):
        return [dict(row) for row in self.db.execute(MANIFEST['queries'][name]['sql'], params)]

    def test_habits_section_lists_scheduled_habits_with_streak_and_done_state(self):
        self.insert('habits', id='gym', title='Gym', weekdays='0,1,2,3,4,5,6', paused=0, started='2026-11-01')
        self.insert('habits', id='read', title='Read', weekdays='3', paused=0, started='2026-11-01')
        self.insert('habits', id='weekend', title='Hike', weekdays='5,6', paused=0, started='2026-11-01')
        self.insert('habits', id='paused', title='Paused', weekdays='0,1,2,3,4,5,6', paused=1, started='2026-11-01')
        for day in ['2026-11-10', '2026-11-11', TODAY]:
            self.insert('checkins', id=f'gym-{day}', habit_id='gym', day=day, status='done')
        rows = {r['id']: r for r in self.query('due_today', TODAY)}
        self.assertEqual(set(rows), {'gym', 'read'})
        self.assertEqual((rows['gym']['is_done'], rows['gym']['streak']), (1, 3))
        self.assertEqual((rows['read']['is_done'], rows['read']['streak']), (0, 0))

    def test_sleep_prompt_only_until_today_is_logged(self):
        self.insert('sleep', id='old', day='2026-11-11', minutes=420)
        self.assertEqual(self.query('today_sleep_prompt', TODAY),
                         [{'id': f'sleep-{TODAY}', 'title': 'Log last night’s sleep'}])
        self.insert('sleep', id='new', day=TODAY, minutes=450)
        self.assertEqual(self.query('today_sleep_prompt', TODAY), [])

    def test_sources_use_existing_check_in_operation(self):
        sources = {s['id']: s for s in MANIFEST['today']}
        self.assertEqual(sources['habits']['section'], 'habits')
        self.assertEqual(sources['habits']['row']['check']['action']['op'], 'checkin')
        self.assertEqual(sources['sleep']['section'], 'wrap_up')
        self.assertEqual(sources['sleep']['row']['open'], {'navigate': {'state': {'tab': 'sleep'}}})
        self.assertIn('today-v1', MANIFEST['requires'])


if __name__ == '__main__':
    unittest.main()

"""Today view (today-v1) queries: sections never overlap and respect the local day."""
from organization_query import OrganizationQuery, MANIFEST

TODAY = '2026-11-12'
OFFSET = 5 * 3600000 + 1800000  # UTC+05:30
# Local midnight of TODAY, in UTC milliseconds.
DAY_START = 1794441600000 - OFFSET


class TodayQuery(OrganizationQuery):
    def add(self, id, **values):
        values.setdefault('title', id)
        self.insert('tasks', id=id, **values)

    def ids(self, name, *params):
        return [row['id'] for row in self.query(name, *params)]

    def test_my_day_is_planned_today_and_earlier_is_separate(self):
        self.add('today', planned_for=TODAY)
        self.add('earlier', planned_for='2026-11-10')
        self.add('future', planned_for='2026-11-13')
        self.add('done-today', planned_for=TODAY, done=1)
        self.add('backlog')
        self.assertEqual(self.ids('today_my_day', TODAY), ['today'])
        self.assertEqual(self.ids('today_earlier', TODAY), ['earlier'])
        self.assertEqual(self.query('today_earlier', TODAY)[0]['planned_label'], 'planned Tue')

    def test_due_excludes_tasks_already_in_my_day_or_earlier(self):
        self.add('overdue', due='2026-11-11')
        self.add('due-today', due=TODAY)
        self.add('due-planned', due=TODAY, planned_for=TODAY)
        self.add('overdue-earlier-plan', due='2026-11-01', planned_for='2026-11-10')
        self.add('due-later-plan', due=TODAY, planned_for='2026-11-14')
        self.add('future', due='2026-11-13')
        self.add('done', due=TODAY, done=1)
        rows = self.query('today_due', TODAY, OFFSET)
        self.assertEqual([r['id'] for r in rows], ['overdue', 'due-today', 'due-later-plan'])
        self.assertEqual(rows[0]['tone'], 'overdue')
        self.assertEqual(rows[0]['time_label'], 'overdue · yesterday')
        self.assertIsNone(rows[1]['tone'])

    def test_due_includes_todays_reminders_by_local_day_and_orders_them(self):
        self.add('late', remind_time='21:00', remind_at=DAY_START + 21 * 3600000)
        self.add('early', due=TODAY, remind_time='08:00', remind_at=DAY_START + 8 * 3600000)
        self.add('tomorrow', remind_time='00:30', remind_at=DAY_START + 86400000 + 1800000)
        self.add('yesterday', remind_time='23:30', remind_at=DAY_START - 1800000)
        self.add('untimed', due=TODAY)
        rows = self.query('today_due', TODAY, OFFSET)
        self.assertEqual([r['id'] for r in rows], ['early', 'late', 'untimed'])
        self.assertEqual(rows[0]['kind'], 'reminder')
        self.assertEqual(rows[0]['sort_ms'], DAY_START + 8 * 3600000)
        self.assertEqual(rows[1]['time_label'], '21:00')
        self.assertIsNone(rows[2]['sort_ms'])

    def test_due_time_already_passed_today_is_overdue(self):
        now = DAY_START + 20 * 3600000 + 8 * 60000  # 20:08 local
        self.add('past', due=TODAY, remind_time='18:00', remind_at=DAY_START + 18 * 3600000)
        self.add('later', due=TODAY, remind_time='22:00', remind_at=DAY_START + 22 * 3600000)
        self.add('untimed', due=TODAY)
        rows = {r['id']: r for r in self.query('today_due', TODAY, OFFSET, now)}
        self.assertEqual(rows['past']['tone'], 'overdue')
        self.assertIsNone(rows['later']['tone'])
        self.assertIsNone(rows['untimed']['tone'])
        self.add('planned-past', planned_for=TODAY, due=TODAY, remind_time='18:00', remind_at=DAY_START + 18 * 3600000)
        self.add('planned-later', planned_for=TODAY, due=TODAY, remind_time='22:00', remind_at=DAY_START + 22 * 3600000)
        tones = {r['id']: r['tone'] for r in self.query('today_my_day', TODAY, now)}
        self.assertEqual(tones, {'planned-past': 'overdue', 'planned-later': None})
        # Without a clock the row keeps its old tone.
        self.assertIsNone({r['id']: r['tone'] for r in self.query('today_my_day', TODAY)}['planned-past'])

    def test_inbox_lists_unscheduled_tasks_without_a_list_and_counts_them(self):
        self.insert('task_lists', id='work', name='Work')
        self.add('loose')
        self.add('listed', list_id='work')
        self.add('planned', planned_for=TODAY)
        self.add('due', due=TODAY)
        self.add('later', due='2026-11-20')
        self.add('closed', done=1)
        self.assertEqual(set(self.ids('today_inbox', TODAY)), {'loose', 'later'})
        self.assertEqual(self.query('today_inbox_count', TODAY), [{'count': 2}])

    def test_done_is_completed_during_the_local_day(self):
        self.add('morning', done=1, completed_at=DAY_START + 3600000)
        self.add('evening', done=1, completed_at=DAY_START + 23 * 3600000)
        self.add('yesterday', done=1, completed_at=DAY_START - 1)
        self.add('tomorrow', done=1, completed_at=DAY_START + 86400000)
        self.add('open', completed_at=DAY_START + 3600000)
        self.assertEqual(self.ids('today_done', TODAY, OFFSET), ['evening', 'morning'])
        self.assertEqual(self.query('today_done_count', TODAY, OFFSET), [{'count': 2}])

    def test_rows_carry_list_tags_and_steps_labels(self):
        self.insert('task_lists', id='home', name='Home')
        self.add('call', planned_for=TODAY, list_id='home', tags='work, calls', remind_time='19:30')
        self.insert('steps', id='s1', task_id='call', title='a', done=1)
        self.insert('steps', id='s2', task_id='call', title='b', done=0)
        row = self.query('today_my_day', TODAY)[0]
        self.assertEqual((row['list_name'], row['tags_label'], row['steps_label'], row['time_label']),
                         ('Home', '#work #calls', '1/2 steps', '19:30'))

    def test_sources_use_existing_operations(self):
        self.assertIn('today-v1', MANIFEST['requires'])
        sources = {s['id']: s for s in MANIFEST['today']}
        self.assertEqual([s['section'] for s in MANIFEST['today']], ['my_day', 'earlier', 'due', 'sort', 'done'])
        my_day = sources['my-day']['row']
        self.assertEqual(my_day['check']['action']['op'], 'toggle')
        self.assertEqual({a['label']: a['action']['args'].get('planned_for') for a in my_day['actions']},
                         {'Tomorrow': '{{$tomorrow}}', 'Not today': None})
        for source in MANIFEST['today']:
            self.assertIn(source['query'], MANIFEST['queries'])
            for action in source['row'].get('actions', []):
                self.assertIn(action['action']['op'], MANIFEST['operations'])


if __name__ == '__main__':
    import unittest
    unittest.main()

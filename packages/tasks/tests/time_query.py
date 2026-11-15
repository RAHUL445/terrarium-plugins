"""Time estimates and tracking in the read model (FIX-18).

`tasks.estimate_min` is a task's own estimate; `time_entries` holds timed
entries (running while `ended_at` is empty) and manual ones. A parent's shown
estimate and tracked time include its subtasks'; a running entry counts the
time that has passed (`now`)."""
import json
from organization_query import OrganizationQuery, MANIFEST

TODAY = '2026-11-12'
# 2026-11-12 10:00:00 UTC, in unix seconds and milliseconds.
T0 = 1794477600
NOW = (T0 + 7 * 60) * 1000  # seven minutes after a timer started at T0


class TimeQuery(OrganizationQuery):
    def task(self, id, **values):
        values.setdefault('title', id)
        self.insert('tasks', id=id, **values)

    def entry(self, task, minutes=None, id=None, started=T0, ended='same', **extra):
        """A finished entry (`minutes`) or, with minutes=None, a running timer."""
        if ended == 'same':
            ended = None if minutes is None else started
        self.insert('time_entries', id=id or f'{task}.{started}.{minutes}', task_id=task, started_at=started,
                    ended_at=ended, minutes=minutes, **extra)

    def one(self, name, *params):
        rows = self.query(name, *params)
        self.assertEqual(len(rows), 1, rows)
        return rows[0]

    def workspace(self, filter='everything', sort='manual', now=NOW):
        return self.query('workspace_tasks', TODAY, filter, None, None, None, None, None, 'any', None, sort, 'list',
                          None, None, None, now)

    def rows(self, **kw):
        return {r['id']: r for r in self.workspace(**kw)}

    # ---- schema -----------------------------------------------------------

    def test_storage_adds_estimate_and_time_entries_in_version_8(self):
        storage = MANIFEST['storage']
        self.assertEqual(storage['version'], 9)  # 8 added time tracking; 9 (FIX-19) only adds settings.auto_archive
        tasks = next(t for t in storage['tables'] if t['name'] == 'tasks')
        estimate = next(c for c in tasks['columns'] if c['name'] == 'estimate_min')
        self.assertEqual((estimate['type'], 'default' in estimate), ('integer', False))
        entries = next(t for t in storage['tables'] if t['name'] == 'time_entries')
        self.assertEqual(entries['delete_policy'], 'trash')
        self.assertEqual([(c['name'], c['type']) for c in entries['columns']],
                         [('task_id', 'text'), ('started_at', 'integer'), ('ended_at', 'integer'),
                          ('minutes', 'integer'), ('note', 'text')])
        self.assertEqual(MANIFEST['version'], '0.12.4')

    def test_new_commands_are_declared_with_descriptions(self):
        ops = MANIFEST['operations']
        for name in ['set_estimate', 'start_timer', 'stop_timer', 'add_time', 'update_time_entry', 'delete_time_entry',
                     'restore_time_entry', 'resume_timer', 'undo_start_timer']:
            self.assertIn(name, ops)
            self.assertGreater(len(ops[name]['description']), 40, name)
        self.assertEqual(ops['delete_time_entry']['effect'], 'destructive')
        self.assertIs(ops['resume_timer']['cli'], False)
        self.assertEqual(MANIFEST['queries']['time_entries']['params'], ['task_id', 'now_ms', 'utc_offset_ms', 'today'])
        for name in ['time_entries', 'running_timer', 'time_summary']:
            self.assertGreater(len(MANIFEST['queries'][name]['description']), 40, name)

    # ---- totals and labels ------------------------------------------------

    def test_task_totals_labels_and_over_estimate(self):
        self.task('a', estimate_min=180)
        self.entry('a', 50)
        self.entry('a', 30, started=T0 + 100)
        row = self.one('task', 'a', NOW)
        self.assertEqual((row['own_estimate_min'], row['estimate_min']), (180, 180))
        self.assertEqual((row['own_tracked_min'], row['tracked_min']), (80, 80))
        self.assertEqual(row['time_label'], '1h 20m / 3h')
        self.assertEqual(row['time_summary'], '1h 20m of 3h')
        self.assertEqual(row['estimate_text'], '3h')
        self.assertIsNone(row['time_tone'])
        self.assertIsNone(row['time_over'])
        self.assertEqual(row['time_title'], '1h 20m tracked of 3h estimated')
        self.assertEqual(row['running'], 0)
        self.assertIsNone(row['running_label'])
        self.task('over', estimate_min=60)
        self.entry('over', 90)
        over = self.one('task', 'over', NOW)
        self.assertEqual((over['time_tone'], over['time_over'], over['time_label']), ('danger', 'Over by 30m', '1h 30m / 1h'))

    def test_estimate_only_tracked_only_and_neither(self):
        self.task('est', estimate_min=45)
        self.task('tracked')
        self.entry('tracked', 5)
        self.task('plain')
        est, tracked, plain = (self.one('task', i, NOW) for i in ('est', 'tracked', 'plain'))
        self.assertEqual((est['time_label'], est['time_summary']), ('0m / 45m', '0m of 45m'))
        self.assertEqual((tracked['time_label'], tracked['time_summary']), ('5m', '5m tracked'))
        self.assertEqual((plain['time_label'], plain['time_summary'], plain['estimate_text']), (None, 'Nothing tracked yet', ''))
        self.assertIsNone(plain['estimate_min'])
        rows = self.rows()
        self.assertEqual([rows[i]['time_label'] for i in ('est', 'tracked', 'plain')], ['0m / 45m', '5m', None])

    def test_estimate_of_zero_minutes_reads_as_none(self):
        self.task('z', estimate_min=0)
        self.assertIsNone(self.one('task', 'z', NOW)['estimate_min'])
        self.assertIsNone(self.rows()['z']['time_label'])

    def test_running_timer_counts_elapsed_time_in_totals(self):
        self.task('a', estimate_min=60)
        self.entry('a', 20)
        self.entry('a', None, started=T0)  # running for 7 minutes at NOW
        row = self.one('task', 'a', NOW, 0)
        self.assertEqual((row['running'], row['running_label']), (1, '● Running'))
        self.assertEqual((row['own_tracked_min'], row['tracked_min']), (27, 27))
        self.assertEqual(row['time_label'], '27m / 1h')
        self.assertEqual(row['timer_note'], 'Running since 10:00 · 7m so far')
        # The same entry a minute later: totals move with the clock.
        later = self.one('task', 'a', NOW + 60_000, 0)
        self.assertEqual(later['tracked_min'], 28)
        # Without a clock a running entry adds nothing (and nothing breaks).
        self.assertEqual(self.one('task', 'a')['tracked_min'], 20)
        # Rounded to the nearest minute, like stopping does.
        self.assertEqual(self.one('task', 'a', (T0 + 89) * 1000)['tracked_min'], 21)
        self.assertEqual(self.one('task', 'a', (T0 + 29) * 1000)['tracked_min'], 20)
        self.assertEqual(self.rows()['a']['tracked_min'], 27)
        self.assertEqual(self.rows()['a']['running'], 1)

    def test_started_time_uses_the_local_offset(self):
        self.task('a')
        self.entry('a', None, started=T0)
        row = self.one('task', 'a', NOW, 5.5 * 3600 * 1000)
        self.assertEqual(row['timer_note'], 'Running since 15:30 · 7m so far')

    def test_parent_rolls_up_subtasks_and_subtasks_do_not_roll_up(self):
        self.task('p', estimate_min=60)
        self.task('c1', parent_id='p', estimate_min=90)
        self.task('c2', parent_id='p')
        self.task('gone', parent_id='p', estimate_min=500, _deleted='hlc')
        self.entry('p', 30)
        self.entry('c1', 45)
        self.entry('c2', 15)
        self.entry('gone', 999, _deleted='hlc')
        self.entry('c2', 999, started=T0 + 5, _deleted='hlc')
        parent = self.one('task', 'p', NOW)
        self.assertEqual((parent['own_estimate_min'], parent['estimate_min']), (60, 150))
        self.assertEqual((parent['own_tracked_min'], parent['tracked_min']), (30, 90))
        self.assertEqual(parent['time_label'], '1h 30m / 2h 30m')
        self.assertEqual(parent['time_subtasks'], 'Includes subtasks: 1h of 1h 30m')
        child = self.one('task', 'c1', NOW)
        self.assertEqual((child['estimate_min'], child['tracked_min']), (90, 45))
        self.assertIsNone(child['time_subtasks'])
        rows = self.rows()
        self.assertEqual((rows['p']['tracked_min'], rows['p']['estimate_min']), (90, 150))
        self.assertEqual(rows['p']['time_label'], '1h 30m / 2h 30m')
        flat = {r['id']: r for r in self.query('tasks', TODAY, 'everything', None, None, NOW)}
        self.assertEqual(flat['c1']['time_label'], '45m / 1h 30m')
        self.assertEqual(flat['p']['time_label'], '1h 30m / 2h 30m')
        self.assertNotIn('c1', rows)  # subtasks live under their parent on the main page

    def test_a_subtask_that_runs_marks_its_parent_and_counts_for_it(self):
        self.task('p')
        self.task('c', parent_id='p')
        self.entry('c', None, started=T0)
        parent = self.one('task', 'p', NOW, 0)
        self.assertEqual((parent['running'], parent['running_sub'], parent['running_label']), (0, 1, '● Running'))
        self.assertEqual(parent['tracked_min'], 7)
        self.assertIsNone(parent['timer_note'])  # its own timer isn't running
        self.assertEqual(self.rows()['p']['running_label'], '● Running')
        self.assertEqual(self.rows()['p']['running'], 0)
        flat = {r['id']: r for r in self.query('tasks', TODAY, 'everything', None, None, NOW)}
        self.assertEqual((flat['c']['running'], flat['c']['running_label']), (1, '● Running'))

    def test_trashed_tasks_and_entries_are_left_out(self):
        self.task('a')
        self.entry('a', 10)
        self.entry('a', 90, started=T0 + 9, _deleted='hlc')
        self.assertEqual(self.one('task', 'a', NOW)['tracked_min'], 10)
        self.assertEqual([r['minutes'] for r in self.query('time_entries', 'a', NOW, 0, TODAY)], [10])
        self.insert('tasks', id='dead', title='Dead', _deleted='hlc')
        self.entry('dead', None, started=T0)
        self.assertEqual(self.query('running_timer', NOW, 0), [])

    # ---- entries ----------------------------------------------------------

    def test_entries_newest_first_with_dates_and_ranges(self):
        self.task('a')
        # 10:00-10:35 today UTC, a manual entry yesterday, a running one.
        self.entry('a', 35, id='timed', started=T0, ended=T0 + 35 * 60, note='Draft')
        self.entry('a', 90, id='manual', started=T0 - 86400 + 7200, note=None)
        self.entry('a', None, id='live', started=T0 + 3600)
        rows = self.query('time_entries', 'a', (T0 + 3600 + 5 * 60) * 1000, 0, TODAY)
        self.assertEqual([r['id'] for r in rows], ['live', 'timed', 'manual'])
        live, timed, manual = rows
        self.assertEqual((live['running'], live['minutes'], live['duration_label']), (1, 5, '5m · running'))
        self.assertEqual(live['range_label'], '11:00 – now')
        self.assertEqual((timed['duration_label'], timed['range_label'], timed['note']), ('35m', '10:00 – 10:35', 'Draft'))
        self.assertEqual((timed['date'], timed['date_label']), (TODAY, 'Today'))
        self.assertEqual((manual['duration_label'], manual['date'], manual['date_label']), ('1h 30m', '2026-11-11', 'Yesterday'))
        self.assertIsNone(manual['range_label'])
        # A date in another year names the year; the offset moves the day.
        old = self.query('time_entries', 'a', NOW, 0, '2027-03-01')
        self.assertIn('Nov 2026', {r['id']: r['date_label'] for r in old}['timed'])
        shifted = {r['id']: r for r in self.query('time_entries', 'a', NOW, 15 * 3600 * 1000, TODAY)}
        self.assertEqual(shifted['timed']['date'], '2026-11-13')

    def test_running_timer_query(self):
        self.assertEqual(self.query('running_timer', NOW, 0), [])
        self.task('a', title='Write report')
        self.task('b', title='Older')
        self.entry('b', None, started=T0 - 600)  # offline duplicate: the newest wins
        self.entry('a', None, started=T0)
        row = self.one('running_timer', NOW, 0)
        self.assertEqual((row['task_id'], row['title']), ('a', 'Write report'))
        self.assertEqual((row['elapsed_min'], row['elapsed_label'], row['started_time']), (7, '7m', '10:00'))
        self.assertEqual(row['label'], '● 7m · Write report')
        self.assertEqual(row['running_count'], 2)
        self.assertEqual(self.one('running_timer', NOW + 3600 * 1000, 0)['elapsed_label'], '1h 7m')

    # ---- summary ----------------------------------------------------------

    def test_time_summary_per_task_and_per_list(self):
        self.insert('task_lists', id='work', name='Work')
        self.task('a', title='Alpha', list_id='work')
        self.task('b', title='Beta', list_id='work')
        self.task('i', title='Inbox task')
        day = lambda d: (T0 + d * 86400)
        self.entry('a', 60, started=day(0))
        self.entry('a', 30, started=day(-1))
        self.entry('b', 15, started=day(0))
        self.entry('i', 120, started=day(-3))
        self.entry('b', 500, started=day(0) + 5, _deleted='hlc')
        self.insert('tasks', id='dead', title='Dead', _deleted='hlc')
        self.entry('dead', 999, started=day(0))
        by_task = self.query('time_summary', '2026-11-11', '2026-11-12', 'task', NOW, 0)
        self.assertEqual([(r['title'], r['minutes'], r['duration_label'], r['entries']) for r in by_task],
                         [('Alpha', 90, '1h 30m', 2), ('Beta', 15, '15m', 1)])
        self.assertEqual(by_task[0]['list_name'], 'Work')
        by_list = self.query('time_summary', '2026-11-09', '2026-11-12', 'list', NOW, 0)
        self.assertEqual([(r['title'], r['minutes'], r['tasks']) for r in by_list],
                         [('Inbox', 120, 1), ('Work', 105, 2)])
        self.assertEqual(by_list[1]['id'], 'work')
        # Open-ended ranges; default grouping is by task.
        self.assertEqual(len(self.query('time_summary', None, None, None, NOW, 0)), 3)
        self.assertEqual([r['title'] for r in self.query('time_summary', '2026-11-12', None, None, NOW, 0)], ['Alpha', 'Beta'])
        self.assertEqual(self.query('time_summary', '2026-12-01', '2026-12-31', 'task', NOW, 0), [])
        # A running timer counts its elapsed time.
        self.entry('i', None, started=T0)
        today = self.query('time_summary', TODAY, TODAY, 'list', NOW, 0)
        self.assertEqual({r['title']: r['minutes'] for r in today}, {'Work': 75, 'Inbox': 7})

    # ---- sort -------------------------------------------------------------

    def test_sort_by_time_tracked_and_by_estimate(self):
        self.task('a', estimate_min=30)
        self.task('b', estimate_min=300)
        self.task('c')
        self.entry('a', 90)
        self.entry('b', 10)
        self.entry('c', 20)
        self.assertEqual([r['id'] for r in self.workspace(sort='tracked')], ['a', 'c', 'b'])
        self.assertEqual([r['id'] for r in self.workspace(sort='estimate')], ['b', 'a', 'c'])

    def test_a_parents_sort_uses_the_rollup(self):
        self.task('p')
        self.task('c', parent_id='p', estimate_min=500)
        self.task('x', estimate_min=60)
        self.entry('c', 200)
        self.entry('x', 100)
        top = [r['id'] for r in self.workspace(sort='tracked')]
        self.assertLess(top.index('p'), top.index('x'))
        self.assertEqual([r['id'] for r in self.workspace(sort='estimate')], ['p', 'x'])

    # ---- other lists ------------------------------------------------------

    def test_tasks_query_and_today_sections_carry_the_time(self):
        self.task('a', estimate_min=60, planned_for=TODAY, due=TODAY)
        self.entry('a', 25)
        rows = {r['id']: r for r in self.query('tasks', TODAY, 'everything', None, None, NOW)}
        self.assertEqual((rows['a']['time_label'], rows['a']['running_label']), ('25m / 1h', None))
        self.assertEqual(self.one('today_my_day', TODAY, NOW)['note_label'], '25m / 1h')
        self.task('steps', planned_for=TODAY)
        self.insert('steps', id='s1', task_id='steps', title='one', done=1)
        self.insert('steps', id='s2', task_id='steps', title='two', done=0)
        self.entry('steps', None, started=T0)
        notes = {r['id']: r['note_label'] for r in self.query('today_my_day', TODAY, NOW)}
        self.assertEqual(notes['steps'], '1/2 steps · ● Running · 7m')
        self.assertEqual(notes['a'], '25m / 1h')
        self.task('later', planned_for='2026-11-01')
        self.entry('later', 5)
        self.assertEqual(self.one('today_earlier', TODAY, NOW)['note_label'], '5m')
        self.task('in', due='2026-12-30')
        self.entry('in', None, started=T0)
        inbox = {r['id']: r for r in self.query('today_inbox', TODAY, NOW)}
        self.assertEqual(inbox['in']['note_label'], '● Running · 7m')
        self.task('quiet', planned_for='2026-12-30', due='2026-12-30')
        self.assertIsNone({r['id']: r for r in self.query('today_inbox', TODAY, NOW)}['quiet']['note_label'])
        self.task('overdue', due='2026-11-01')
        self.entry('overdue', 3)
        due = {r['id']: r for r in self.query('today_due', TODAY, 0, NOW)}
        self.assertEqual(due['overdue']['note_label'], '3m')

    # ---- export -----------------------------------------------------------

    def test_csv_export_has_estimate_and_tracked_columns(self):
        export = MANIFEST['export'][0]
        self.assertEqual(export['columns'][-2:], ['estimate', 'tracked'])
        self.task('a', title='Planned', estimate_min=90)
        self.entry('a', 45)
        self.entry('a', 60, started=T0 + 9)
        self.task('p', title='Parent', estimate_min=30)
        self.task('c', title='Child', parent_id='p')
        self.entry('c', 20)
        self.task('plain', title='Plain')
        rows = {r['title']: r for r in self.query('export_tasks', NOW)}
        self.assertEqual((rows['Planned']['estimate'], rows['Planned']['tracked']), ('1h 30m', '1h 45m'))
        # Own values only, so a spreadsheet can add the column up.
        self.assertEqual((rows['Parent']['estimate'], rows['Parent']['tracked']), ('30m', None))
        self.assertEqual((rows['Child']['estimate'], rows['Child']['tracked']), (None, '20m'))
        self.assertEqual((rows['Plain']['estimate'], rows['Plain']['tracked']), (None, None))
        self.entry('plain', None, started=T0)
        self.assertEqual({r['title']: r for r in self.query('export_tasks', NOW)}['Plain']['tracked'], '7m')
        for column in export['columns']:
            self.assertIn(column, rows['Planned'])

    def test_manifest_json_round_trips_for_the_importer(self):
        # The importer maps by header, so old CSVs without the new columns still import.
        importer = MANIFEST['import'][0]
        self.assertNotIn('estimate', importer['fields'])
        self.assertNotIn('tracked', importer['fields'])


if __name__ == '__main__':
    import unittest
    unittest.main()

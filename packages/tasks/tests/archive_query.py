"""Archive ("Logbook") in the read model (FIX-19).

`tasks.cleared_at` is the archive marker (unix ms; non-null = archived).
Archived tasks leave the lists but still count in reviews, reports and
exports; the `archived` query lists them, newest completion first, grouped by
Today / Yesterday / This week / month."""
import calendar
from organization_query import OrganizationQuery, MANIFEST

TODAY = '2026-11-12'
NOW = calendar.timegm((2026, 11, 12, 12, 0, 0)) * 1000


def ms(date, time='10:00:00'):
    y, m, d = (int(x) for x in date.split('-'))
    hh, mm, ss = (int(x) for x in time.split(':'))
    return calendar.timegm((y, m, d, hh, mm, ss)) * 1000


class ArchiveQuery(OrganizationQuery):
    def task(self, id, **values):
        values.setdefault('title', id)
        self.insert('tasks', id=id, **values)

    def done(self, id, completed, archived=True, **values):
        """A done task completed at `completed` (ms or None); archived by default."""
        values.setdefault('cleared_at', ms('2026-11-12') if archived else None)
        self.task(id, done=1, completed_at=completed, **values)

    def archived(self, frm=None, to=None, list_id=None, q=None, today=TODAY, now=NOW, offset=0):
        return self.query('archived', frm, to, list_id, q, today, now, offset)

    def ids(self, rows):
        return [r['id'] for r in rows]

    def workspace(self, include=None, filter='everything'):
        params = [None] * len(MANIFEST['queries']['workspace_tasks']['params'])
        names = MANIFEST['queries']['workspace_tasks']['params']
        for key, value in {'today': TODAY, 'filter': filter, 'now': NOW, 'tags_mode': 'any',
                           'sort': 'manual', 'grouping': 'list', 'include_archived': include}.items():
            params[names.index(key)] = value
        return self.query('workspace_tasks', *params)

    # ---- schema and declarations -------------------------------------------

    def test_storage_version_9_adds_only_the_auto_archive_column(self):
        storage = MANIFEST['storage']
        self.assertEqual(storage['version'], 9)
        settings = next(t for t in storage['tables'] if t['name'] == 'settings')
        self.assertEqual([(c['name'], c['type']) for c in settings['columns']],
                         [('ask_note', 'text'), ('note_tags', 'text'), ('auto_archive', 'text')])
        tasks = next(t for t in storage['tables'] if t['name'] == 'tasks')
        self.assertIn('cleared_at', [c['name'] for c in tasks['columns']])  # the existing marker
        self.assertEqual(MANIFEST['version'], '0.12.4')
        self.assertNotIn('jobs', MANIFEST)  # a job would make users approve the plugin again

    def test_new_commands_and_queries_are_declared(self):
        ops = MANIFEST['operations']
        for name in ['archive', 'unarchive', 'archive_done', 'archive_settings', 'auto_archive', 'clear_done']:
            self.assertIn(name, ops)
            self.assertGreater(len(ops[name]['description']), 40, name)
        self.assertIs(ops['auto_archive']['cli'], False)
        self.assertEqual(ops['archive']['params']['id']['required'], True)
        self.assertEqual(ops['archive_settings']['params']['auto']['values'], ['never', 'end_of_day', '7d', '30d'])
        self.assertEqual(ops['clear_done']['confirm'], ops['archive_done']['confirm'])
        self.assertIn('Archive all completed tasks?', ops['clear_done']['confirm'])
        queries = MANIFEST['queries']
        self.assertEqual(queries['archived']['params'],
                         ['from', 'to', 'list_id', 'q', 'today', 'now_ms', 'utc_offset_ms'])
        self.assertIn('what did I finish in October', queries['archived']['description'])
        self.assertEqual(queries['archive_stats']['params'], ['today', 'utc_offset_ms'])
        self.assertEqual(queries['archive_settings']['params'], [])
        for name in ['tasks', 'workspace_tasks']:
            # workspace_tasks gained `nesting` (FIX-21) after it.
            self.assertIn('include_archived', queries[name]['params'][-2:], name)
            self.assertEqual(queries[name]['types']['include_archived']['type'], 'boolean', name)

    # ---- the archived query -------------------------------------------------

    def test_lists_only_archived_live_tasks_newest_completion_first(self):
        self.done('old', ms('2026-11-01'))
        self.done('new', ms('2026-11-11'))
        self.done('mid', ms('2026-11-05'))
        self.done('visible', ms('2026-11-12'), archived=False)  # done but not archived
        self.task('open')
        self.done('trashed', ms('2026-11-10'), _deleted=1)
        self.assertEqual(self.ids(self.archived()), ['new', 'mid', 'old'])

    def test_group_label_boundaries(self):
        for id, date, time in [
            ('today-late', '2026-11-12', '23:59:59'),
            ('yesterday-late', '2026-11-11', '23:59:59'),
            ('yesterday-early', '2026-11-11', '00:00:00'),
            ('week-first', '2026-11-10', '23:59:59'),   # two days ago
            ('week-last', '2026-11-06', '00:00:00'),    # six days ago: the seventh local day
            ('month-first', '2026-11-05', '23:59:59'),  # seven days ago: out of the week
            ('prev-month', '2026-10-31', '12:00:00'),
            ('prev-year', '2025-12-01', '12:00:00'),
        ]:
            self.done(id, ms(date, time))
        self.done('undated', None)
        got = {r['id']: r['group_label'] for r in self.archived()}
        self.assertEqual(got, {
            'today-late': 'Today',
            'yesterday-late': 'Yesterday',
            'yesterday-early': 'Yesterday',
            'week-first': 'This week',
            'week-last': 'This week',
            'month-first': 'November 2026',
            'prev-month': 'October 2026',
            'prev-year': 'December 2025',
            'undated': 'Earlier',
        })
        # Newest first, and the undated one last: groups stay contiguous.
        labels = [r['group_label'] for r in self.archived()]
        self.assertEqual(labels[0], 'Today')
        self.assertEqual(labels[-1], 'Earlier')
        self.assertEqual(sorted(set(labels), key=labels.index), list(dict.fromkeys(labels)))

    def test_local_dates_follow_the_utc_offset(self):
        # 20:00 UTC on the 11th is 01:30 on the 12th at +05:30.
        self.done('late', ms('2026-11-11', '20:00:00'))
        row = self.archived(offset=19800000)[0]
        self.assertEqual((row['completed_date'], row['group_label'], row['completed_label']),
                         ('2026-11-12', 'Today', '12 Nov'))
        row = self.archived(offset=0)[0]
        self.assertEqual((row['completed_date'], row['group_label']), ('2026-11-11', 'Yesterday'))
        # The same task seen from the west is on the 11th too.
        row = self.archived(offset=-5 * 3600000)[0]
        self.assertEqual(row['completed_date'], '2026-11-11')

    def test_completed_label_shows_the_year_only_when_it_differs(self):
        self.done('a', ms('2026-10-12'))
        self.done('b', ms('2025-10-12'))
        labels = {r['id']: r['completed_label'] for r in self.archived()}
        self.assertEqual(labels, {'a': '12 Oct', 'b': '12 Oct 2025'})

    def test_from_to_are_local_completion_dates_inclusive(self):
        for id, date in [('a', '2026-09-30'), ('b', '2026-10-01'), ('c', '2026-10-31'), ('d', '2026-11-01')]:
            self.done(id, ms(date, '23:30:00'))
        self.assertEqual(self.ids(self.archived('2026-10-01', '2026-10-31')), ['c', 'b'])
        self.assertEqual(self.ids(self.archived('2026-10-31', None)), ['d', 'c'])
        self.assertEqual(self.ids(self.archived(None, '2026-10-01')), ['b', 'a'])
        self.assertEqual(self.ids(self.archived('', '')), ['d', 'c', 'b', 'a'])
        # At +05:30, 23:30 UTC on the 30th is already 1 October and on the 31st already 1 November.
        self.assertEqual(self.ids(self.archived('2026-10-01', '2026-10-31', offset=19800000)), ['b', 'a'])

    def test_search_matches_title_notes_tags_and_the_note_on_finishing(self):
        self.done('t', ms('2026-11-01'), title='Quarterly Report')
        self.done('n', ms('2026-11-02'), notes='mention of the budget')
        self.done('g', ms('2026-11-03'), tags='finance, budget')
        self.done('d', ms('2026-11-04'), done_note='Spent less than the BUDGET')
        self.done('x', ms('2026-11-05'), title='Unrelated')
        self.assertEqual(self.ids(self.archived(q='budget')), ['d', 'g', 'n'])
        self.assertEqual(self.ids(self.archived(q='quarterly')), ['t'])
        self.assertEqual(self.ids(self.archived(q='')), ['x', 'd', 'g', 'n', 't'])
        self.assertEqual(self.archived(q='nothing like this'), [])
        # Open or unarchived tasks never match.
        self.task('o', title='budget open')
        self.done('u', ms('2026-11-06'), title='budget visible', archived=False)
        self.assertEqual(self.ids(self.archived(q='budget')), ['d', 'g', 'n'])

    def test_list_filter_with_inbox_and_deleted_lists(self):
        self.insert('task_lists', id='work', name='Work')
        self.insert('task_lists', id='home', name='Home')
        self.done('w', ms('2026-11-01'), list_id='work')
        self.done('h', ms('2026-11-02'), list_id='home')
        self.done('i', ms('2026-11-03'))
        self.assertEqual(self.ids(self.archived(list_id='work')), ['w'])
        self.assertEqual(self.ids(self.archived(list_id='@inbox')), ['i'])
        self.assertEqual(self.ids(self.archived(list_id='')), ['i', 'h', 'w'])
        names = {r['id']: r['list_name'] for r in self.archived()}
        self.assertEqual(names, {'w': 'Work', 'h': 'Home', 'i': 'Inbox'})
        self.assertEqual(self.ids(self.archived(list_id='work', q='nothing')), [])
        self.assertEqual(self.ids(self.archived('2026-11-02', '2026-11-03', 'home')), ['h'])

    def test_rows_carry_note_time_subtasks_and_repeat_labels(self):
        self.done('p', ms('2026-11-11'), title='Parent', estimate_min=60, repeat='weekly',
                  done_note='First line\nSecond line')
        self.done('c1', ms('2026-11-11'), parent_id='p', estimate_min=30)
        self.task('c2', parent_id='p')  # still open
        self.insert('time_entries', id='e1', task_id='p', started_at=1, ended_at=1, minutes=50)
        self.insert('time_entries', id='e2', task_id='c1', started_at=1, ended_at=1, minutes=40)
        row = next(r for r in self.archived() if r['id'] == 'p')
        self.assertEqual(row['done_note'], 'First line\nSecond line')
        self.assertEqual(row['done_line'], 'First line')
        self.assertEqual((row['tracked_min'], row['estimate_min']), (90, 90))
        self.assertEqual((row['time_label'], row['time_tone']), ('1h 30m / 1h 30m', None))
        self.assertEqual(row['subtasks_label'], '1/2')
        self.assertEqual(row['repeat_label'], 'Weekly')
        child = next(r for r in self.archived() if r['id'] == 'c1')
        self.assertEqual((child['time_label'], child['parent_label'], child['subtasks_label']),
                         ('40m / 30m', '\u21b3 Parent', None))
        self.assertEqual(child['time_tone'], 'danger')
        self.assertIsNone(child['repeat_label'])
        self.assertIsNone(row['parent_label'])

    def test_result_is_limited_to_500(self):
        for i in range(505):
            self.done(f'a{i:03d}', ms('2026-11-01') + i)
        rows = self.archived()
        self.assertEqual(len(rows), 500)
        self.assertEqual(rows[0]['id'], 'a504')

    # ---- header stats and settings --------------------------------------------

    def test_stats_count_done_and_tracked_this_local_month(self):
        self.done('a', ms('2026-11-03'))                    # archived, this month
        self.done('b', ms('2026-11-10'), archived=False)    # not archived, this month
        self.done('c', ms('2026-10-31'))                    # last month
        self.done('d', ms('2026-11-04'), _deleted=1)        # trashed
        self.task('e')                                       # open
        self.insert('time_entries', id='t1', task_id='a', started_at=ms('2026-11-03') // 1000, ended_at=1, minutes=125)
        self.insert('time_entries', id='t2', task_id='e', started_at=ms('2026-11-05') // 1000, ended_at=1, minutes=75)
        self.insert('time_entries', id='t3', task_id='c', started_at=ms('2026-10-31') // 1000, ended_at=1, minutes=999)
        self.insert('time_entries', id='t4', task_id='a', started_at=ms('2026-11-06') // 1000, ended_at=1, minutes=5,
                    _deleted=1)
        row = self.query('archive_stats', TODAY, 0)[0]
        self.assertEqual((row['done_month'], row['tracked_month']), (2, 200))
        self.assertEqual(row['label'], '2 done this month \u00b7 3h 20m tracked')

    def test_stats_without_tracked_time_and_local_month_edges(self):
        self.done('a', ms('2026-10-31', '20:00:00'))
        self.assertEqual(self.query('archive_stats', TODAY, 0)[0]['label'], '0 done this month')
        # +05:30 pushes 20:00 UTC on 31 Oct into November.
        row = self.query('archive_stats', TODAY, 19800000)[0]
        self.assertEqual((row['done_month'], row['label']), (1, '1 done this month'))

    def test_archive_settings_defaults_to_never_and_reads_the_oldest_row(self):
        self.assertEqual(self.query('archive_settings'), [{'auto': 'never'}])
        self.insert('settings', id='b', ask_note='always', auto_archive='30d')
        self.insert('settings', id='a', ask_note='never', auto_archive='7d')
        self.assertEqual(self.query('archive_settings'), [{'auto': '7d'}])
        self.db.execute("UPDATE settings SET auto_archive = '' WHERE id = 'a'")
        self.assertEqual(self.query('archive_settings'), [{'auto': 'never'}])

    # ---- the task query ---------------------------------------------------------

    def test_task_reports_archived_state_and_label(self):
        self.done('a', ms('2026-11-11'), cleared_at=ms('2026-10-12', '08:00:00'))
        self.done('b', ms('2026-11-11'), archived=False)
        self.task('c')
        self.done('d', ms('2025-01-01'), cleared_at=ms('2025-03-04'))
        a = self.query('task', 'a', NOW, 0)[0]
        self.assertEqual((a['archived'], a['archived_label'], a['can_archive'], a['can_start_timer']),
                         (1, 'Archived 12 Oct', 0, 0))
        b = self.query('task', 'b', NOW, 0)[0]
        self.assertEqual((b['archived'], b['archived_label'], b['can_archive'], b['can_start_timer']),
                         (0, None, 1, 1))
        c = self.query('task', 'c', NOW, 0)[0]
        self.assertEqual((c['archived'], c['can_archive'], c['can_start_timer']), (0, 0, 1))
        d = self.query('task', 'd', NOW, 0)[0]
        self.assertEqual(d['archived_label'], 'Archived 4 Mar 2025')
        # A running timer hides Start timer (it shows Stop instead).
        self.insert('time_entries', id='run', task_id='c', started_at=NOW // 1000, ended_at=None)
        self.assertEqual(self.query('task', 'c', NOW, 0)[0]['can_start_timer'], 0)
        # The label is the local date: +05:30 moves 22:00 UTC to the next day.
        self.done('e', ms('2026-11-01'), cleared_at=ms('2026-10-12', '22:00:00'))
        self.assertEqual(self.query('task', 'e', NOW, 19800000)[0]['archived_label'], 'Archived 13 Oct')

    # ---- who sees archived tasks ----------------------------------------------------

    def seed_lists(self):
        self.task('open', title='Open task')
        self.done('shown', ms('2026-11-10'), archived=False, title='Done and shown')
        self.done('hidden', ms('2026-11-10'), title='Archived task')
        self.done('hidden-trash', ms('2026-11-10'), title='Archived and trashed', _deleted=1)

    def test_tasks_hides_archived_unless_asked(self):
        self.seed_lists()
        for flag in (None, 0, '0', 'false'):
            rows = self.query('tasks', TODAY, 'everything', None, None, None, flag)
            self.assertEqual(sorted(self.ids(rows)), ['open', 'shown'], flag)
        for flag in (1, '1', 'true'):
            rows = self.query('tasks', TODAY, 'everything', None, None, None, flag)
            self.assertEqual(sorted(self.ids(rows)), ['hidden', 'open', 'shown'], flag)
        done_only = self.query('tasks', TODAY, 'done', None, None, None, 1)
        self.assertEqual(sorted(self.ids(done_only)), ['hidden', 'shown'])
        self.assertEqual(self.ids(self.query('tasks', TODAY, 'done')), ['shown'])
        # q still narrows it.
        found = self.query('tasks', TODAY, 'everything', 'archived', None, None, 1)
        self.assertEqual(self.ids(found), ['hidden'])

    def test_workspace_tasks_hides_archived_unless_asked(self):
        self.seed_lists()
        for flag in (None, 0):
            self.assertEqual(sorted(self.ids(self.workspace(flag))), ['open', 'shown'], flag)
        self.assertEqual(sorted(self.ids(self.workspace(1))), ['hidden', 'open', 'shown'])
        self.assertEqual(sorted(self.ids(self.workspace(True, 'done'))), ['hidden', 'shown'])

    def test_an_open_subtask_of_an_archived_parent_shows_as_top_level(self):
        self.done('parent', ms('2026-11-10'))
        self.task('child', parent_id='parent')
        row = next(r for r in self.query('tasks', TODAY, 'everything') if r['id'] == 'child')
        self.assertIsNone(row['parent_label'])
        ws = next(r for r in self.workspace(None) if r['id'] == 'child')
        self.assertIsNone(ws['parent_label'])

    def test_search_index_counts_and_other_list_queries_still_exclude_archived(self):
        self.seed_lists()
        self.assertEqual(self.ids(self.query('search', 'Archived')), [])
        self.assertEqual(self.ids(self.query('search', 'Done and')), ['shown'])
        self.assertEqual(self.ids(self.query('search', 'task')), ['open'])
        indexed = self.query('index_tasks', '["open","shown","hidden"]')
        self.assertEqual(sorted(self.ids(indexed)), ['open', 'shown'])
        self.assertEqual(self.query('counts', TODAY)[0]['done'], 1)
        self.assertEqual(self.query('nav_counts', TODAY)[0]['open'], 1)
        self.assertEqual(self.query('summary', TODAY)[0]['open'], 1)
        self.assertEqual(self.ids(self.query('task_choices', 'open', 'Done and')), ['shown'])
        self.assertEqual(self.query('task_choices', 'open', 'Archived'), [])
        # Completed today: the Today page's Done list leaves archived ones out.
        self.done('today-shown', ms(TODAY), archived=False)
        self.done('today-archived', ms(TODAY))
        self.assertEqual(self.ids(self.query('today_done', TODAY, 0)), ['today-shown'])
        self.assertEqual(self.query('today_done_count', TODAY, 0)[0].popitem()[1], 1)

    def test_reviews_reports_and_exports_still_count_archived_tasks(self):
        self.done('arch', ms('2026-11-10'), title='Archived win', done_note='went well')
        self.done('plain', ms('2026-11-10', '11:00:00'), archived=False, title='Plain win')
        self.insert('time_entries', id='te', task_id='arch', started_at=ms('2026-11-10') // 1000, ended_at=1,
                    minutes=25)
        review = self.query('review_tasks', ms('2026-11-10', '00:00:00'), ms('2026-11-11', '00:00:00'), 100, 0,
                            'completed_at', 'asc')
        self.assertEqual(self.ids(review), ['arch', 'plain'])
        summary = self.query('time_summary', '2026-11-01', '2026-11-30', 'task', NOW, 0)
        self.assertEqual([(r['id'], r['minutes']) for r in summary], [('arch', 25)])
        by_list = self.query('time_summary', '2026-11-01', '2026-11-30', 'list', NOW, 0)
        self.assertEqual([(r['title'], r['minutes']) for r in by_list], [('Inbox', 25)])
        export = {r['title']: r for r in self.query('export_tasks', NOW)}
        self.assertEqual(export['Archived win']['done'], 'true')
        self.assertIn('Plain win', export)
        refs = self.query('ref_tasks', '["arch","plain"]')
        self.assertEqual(sorted(self.ids(refs)), ['arch', 'plain'])

    def test_done_note_search_covers_the_archive_but_trash_stays_separate(self):
        self.done('a', ms('2026-11-10'), done_note='kept')
        self.done('b', ms('2026-11-10'), _deleted=1)
        self.assertEqual(self.ids(self.archived()), ['a'])
        self.assertEqual(self.ids(self.query('trash')), ['b'])


if __name__ == '__main__':
    import unittest
    unittest.main()

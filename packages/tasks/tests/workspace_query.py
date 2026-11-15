"""Behavioral coverage for the workspace_tasks read model in the manifest."""
import json
from pathlib import Path

from organization_query import OrganizationQuery, MANIFEST

MANIFEST_DIR = Path(__file__).resolve().parents[1]


class WorkspaceQuery(OrganizationQuery):
    TODAY = '2026-11-12'

    def workspace(self, *, filter='everything', q=None, tags=None, lists=None,
                  groups=None, statuses=None, tags_mode='any', category=None,
                  sort='manual', grouping='list', page_focus=None,
                  page_size=None, offset=None):
        return self.query('workspace_tasks', self.TODAY, filter, q, tags, lists,
                          groups, statuses, tags_mode, category, sort, grouping,
                          page_focus, page_size, offset)

    def tree_rows(self, filter):
        names = MANIFEST['queries']['workspace_tasks']['params']
        params = [None] * len(names)
        for key, value in {'today': self.TODAY, 'filter': filter, 'tags_mode': 'any', 'sort': 'manual',
                           'grouping': 'list', 'nesting': 'tree'}.items():
            params[names.index(key)] = value
        return self.query('workspace_tasks', *params)

    def test_page_focus_intersects_saved_search_before_limit(self):
        for i in range(550):
            self.add_task(f'noise-{i}', title='review unrelated')
        self.add_task('target', title='review', notes='launch')
        self.add_task('only-focus', title='launch')
        rows = self.workspace(q='review', page_focus='launch')
        self.assertEqual([row['id'] for row in rows], ['target'])

    def add_task(self, id, title=None, **values):
        self.insert('tasks', id=id, title=title or id, **values)

    def test_board_choices_retain_archived_and_missing_assignments(self):
        self.insert('task_lists', id='old-list', name='Old', archived=1)
        self.insert('task_groups', id='old-group', name='Old group', archived=1)
        self.insert('task_statuses', id='old-status', name='Old status', category='active', archived=1)
        self.add_task('archived', list_id='old-list', group_id='old-group', status_id='old-status')
        self.add_task('missing', list_id='missing-list', group_id='missing-group', status_id='missing-status')
        for kind in ['list', 'group', 'status']:
            choices = {row['value']: row['label'] for row in self.query('view_' + kind + '_choices')}
            self.assertIn('archived', choices['old-' + kind])
            self.assertIn('Unavailable', choices['missing-' + kind])

    def test_my_day_includes_today_and_earlier_plans_only(self):
        for id, planned_for in [('earlier', '2026-11-11'), ('today', self.TODAY),
                                ('backlog', None), ('future', '2026-11-13')]:
            self.add_task(id, planned_for=planned_for)
        self.add_task('completed', planned_for=self.TODAY, done=1)
        self.assertEqual({r['id'] for r in self.workspace(filter='my_day')}, {'earlier', 'today'})

    def test_ready_excludes_waiting_status_and_future_plan(self):
        self.insert('task_lists', id='l', name='Work')
        self.insert('task_statuses', id='ready', list_id='l', name='Ready', category='not_started', ready=1)
        self.insert('task_statuses', id='waiting', list_id='l', name='Waiting', category='not_started', ready=0)
        self.add_task('ready', list_id='l', status_id='ready')
        self.add_task('waiting', list_id='l', status_id='waiting')
        self.add_task('future', planned_for='2026-11-13')
        self.assertEqual({r['id'] for r in self.workspace(filter='ready')}, {'ready'})

    def test_due_today_excludes_undated_task(self):
        self.add_task('due', due=self.TODAY)
        self.add_task('undated')
        self.assertEqual([r['id'] for r in self.workspace(filter='today')], ['due'])

    def test_list_group_status_are_or_within_each_field_and_and_between_fields(self):
        self.insert('task_lists', id='l1', name='One')
        self.insert('task_lists', id='l2', name='Two')
        self.insert('task_groups', id='g1', list_id='l1', name='G1')
        self.insert('task_groups', id='g2', list_id='l2', name='G2')
        self.insert('task_statuses', id='s1', list_id='l1', name='S1', category='not_started')
        self.insert('task_statuses', id='s2', list_id='l2', name='S2', category='not_started')
        self.add_task('match', list_id='l1', group_id='g1', status_id='s1')
        self.add_task('match-alternate-values', list_id='l2', group_id='g2', status_id='s2')
        self.add_task('other-group', list_id='l1', group_id='other', status_id='s1')
        self.add_task('other-status', list_id='l1', group_id='g1', status_id='other')
        rows = self.workspace(lists='l1,l2', groups='g1,g2', statuses='s1,s2')
        self.assertEqual({r['id'] for r in rows}, {'match', 'match-alternate-values'})

    def test_tags_any_and_all_modes(self):
        self.add_task('both', tags='work, urgent')
        self.add_task('work', tags='work')
        self.add_task('urgent', tags='urgent')
        self.assertEqual({r['id'] for r in self.workspace(tags='work, urgent')}, {'both', 'work', 'urgent'})
        self.assertEqual({r['id'] for r in self.workspace(tags='work, urgent', tags_mode='all')}, {'both'})

    def test_filters_apply_before_limit(self):
        for index in range(510):
            self.add_task(f'noise-{index:03}', title='noise')
        self.add_task('target', title='needle task')
        rows = self.workspace(q='needle')
        self.assertEqual([r['id'] for r in rows], ['target'])

    def test_planning_label_is_independent_of_deadline_label(self):
        self.add_task('planned', planned_for=self.TODAY, due='2026-12-01')
        row = self.workspace(grouping='date')[0]
        self.assertEqual(row['workspace_group'], 'Planned today')
        self.assertEqual(row['due'], '2026-12-01')
        self.assertEqual(row['due_label'], '1 Dec')

    def test_undated_tasks_have_no_due_chip(self):
        self.add_task('undated')
        self.add_task('dated', due='2026-12-01')
        rows = {r['id']: r for r in self.workspace()}
        self.assertIsNone(rows['undated']['due_label'])
        self.assertIsNone(rows['undated']['due_tone'])
        self.assertEqual(rows['dated']['due_label'], '1 Dec')
        # The section still groups undated tasks together.
        self.assertEqual(rows['undated']['section'], 'No date')
        widget = {r['id']: r for r in self.query('tasks', self.TODAY, 'everything')}
        self.assertIsNone(widget['undated']['due_label'])

    def test_due_time_already_passed_today_is_overdue(self):
        at = 1794441600000  # 2026-11-12 00:00 UTC
        self.add_task('past', due=self.TODAY, remind_time='18:00', remind_at=at + 18 * 3600000)
        self.add_task('later', due=self.TODAY, remind_time='22:00', remind_at=at + 22 * 3600000)
        self.add_task('untimed', due=self.TODAY)
        self.add_task('yesterday', due='2026-11-11')
        now = at + 20 * 3600000 + 8 * 60000  # 20:08
        rows = self.query('workspace_tasks', self.TODAY, 'everything', None, None, None, None, None,
                          'any', None, 'manual', 'list', None, None, None, now)
        tones = {r['id']: r['due_tone'] for r in rows}
        self.assertEqual(tones, {'past': 'danger', 'later': 'accent', 'untimed': 'accent', 'yesterday': 'danger'})
        # Without a clock (older hosts) a task due today keeps the accent tone.
        tones = {r['id']: r['due_tone'] for r in self.workspace()}
        self.assertEqual(tones['past'], 'accent')

    def test_home_widget_marks_due_time_already_passed_today_as_overdue(self):
        at = 1794441600000  # 2026-11-12 00:00 UTC
        self.add_task('past', due=self.TODAY, remind_time='18:00', remind_at=at + 18 * 3600000)
        self.add_task('later', due=self.TODAY, remind_time='22:00', remind_at=at + 22 * 3600000)
        self.add_task('untimed', due=self.TODAY)
        self.add_task('yesterday', due='2026-11-11')
        now = at + 20 * 3600000 + 8 * 60000  # 20:08
        rows = self.query('tasks', self.TODAY, 'today', None, None, now)
        tones = {r['id']: r['due_tone'] for r in rows}
        self.assertEqual(tones, {'past': 'danger', 'later': 'accent', 'untimed': 'accent', 'yesterday': 'danger'})
        # The clock is optional: without it a task due today keeps the accent tone.
        tones = {r['id']: r['due_tone'] for r in self.query('tasks', self.TODAY, 'today')}
        self.assertEqual(tones['past'], 'accent')
        # The widget passes the host clock.
        widget = json.load(open(MANIFEST_DIR / 'ui' / 'widget-today.json'))
        self.assertEqual(widget['body']['query'], 'tasks')
        self.assertEqual(widget['body']['params']['now'], '{{$now}}')

    def test_saved_view_board_columns_match_lanes(self):
        view = MANIFEST['queries']['workspace_tasks']['view']
        columns = {c['key']: c for c in view['columns']}
        self.add_task('doing', in_progress=1, urgent=1, important=1)
        row = self.workspace()[0]
        self.assertIn(row['board_lane'], columns['board_lane']['values'])
        self.assertIn(row['matrix_lane'], columns['matrix_lane']['values'])
        self.assertEqual((row['board_lane'], row['matrix_lane']), ('doing', 'do'))
        self.assertEqual(columns['board_lane']['move']['op'], 'move')

    def test_manual_title_and_priority_sorts(self):
        self.add_task('z', title='Zulu', priority=1, sort_order=1)
        self.add_task('a', title='Alpha', priority=3, sort_order=2)
        self.assertEqual([r['id'] for r in self.workspace(sort='manual')], ['z', 'a'])
        self.assertEqual([r['id'] for r in self.workspace(sort='title')], ['a', 'z'])
        self.assertEqual([r['id'] for r in self.workspace(sort='priority')], ['a', 'z'])

    def test_workspace_pagination_reaches_tasks_beyond_legacy_500_limit(self):
        for index in range(625):
            self.add_task(f'task-{index:04d}')
        page = self.workspace(page_size=101, offset=500)
        tail = self.workspace(page_size=101, offset=600)
        self.assertEqual(len(page), 101)
        self.assertEqual([row['id'] for row in page[:2]], ['task-0124', 'task-0123'])
        self.assertEqual([row['id'] for row in tail], [f'task-{i:04d}' for i in range(24, -1, -1)])
        self.assertEqual(len({row['id'] for row in page + tail}), 125)
        self.assertEqual(len(self.workspace()), 500)
        self.assertEqual(MANIFEST['queries']['workspace_tasks']['view']['params']['page_size'], 500)

    def test_workspace_pagination_metadata_and_completion_followup(self):
        view = MANIFEST['queries']['workspace_tasks']['view']
        self.assertEqual(view['pagination']['page_size_param'], 'page_size')
        self.assertEqual(view['pagination']['max_page_size'], 1000)
        self.assertEqual(view['params']['page_size'], 500)
        self.assertEqual(view['row_action']['action']['op'], 'toggle')
        self.assertEqual(view['row_action']['followup']['if'], '{{result.ask_note}}')
        self.assertEqual(view['row_action']['followup']['action']['navigate']['item'], '{{result.ask_note}}')

    def test_nav_counts_match_each_smart_view(self):
        self.insert('task_lists', id='l', name='Work')
        self.insert('task_statuses', id='waiting', list_id='l', name='Waiting', category='not_started', ready=0)
        self.add_task('earlier', planned_for='2026-11-11')
        self.add_task('planned', planned_for=self.TODAY, list_id='l')
        self.add_task('future-plan', planned_for='2026-11-13', due='2026-11-20')
        self.add_task('overdue', due='2026-11-01', list_id='l', status_id='waiting')
        self.add_task('due-today', due=self.TODAY)
        self.add_task('completed', planned_for=self.TODAY, done=1)
        self.add_task('cleared', done=1, cleared_at=1)
        self.add_task('deleted', _deleted=1)
        counts = self.query('nav_counts', self.TODAY)[0]
        for key, filter in [('my_day', 'my_day'), ('inbox', 'inbox'), ('today', 'today'),
                            ('upcoming', 'upcoming'), ('ready', 'ready'), ('open', 'all')]:
            self.assertEqual(counts[key], len(self.workspace(filter=filter)), key)
        self.assertEqual(counts['my_day'], 2)
        self.assertEqual(counts['ready'], 3)

    def test_nav_counts_are_zero_without_tasks(self):
        counts = self.query('nav_counts', self.TODAY)[0]
        self.assertEqual(set(counts.values()), {0})

    def test_nav_lists_count_open_tasks_in_active_lists(self):
        self.insert('task_lists', id='a', name='Alpha', position=1, color='#0f766e')
        self.insert('task_lists', id='b', name='Beta', position=2)
        self.insert('task_lists', id='old', name='Old', archived=1)
        self.add_task('open-a', list_id='a')
        self.add_task('done-a', list_id='a', done=1)
        self.add_task('gone-a', list_id='a', _deleted=1)
        self.add_task('inbox')
        self.add_task('sub-a', list_id='a', parent_id='open-a')
        rows = self.query('nav_lists')
        # Inbox is the first list; subtasks count like any open task.
        self.assertEqual([(r['id'], r['open']) for r in rows], [('inbox', 1), ('a', 2), ('b', 0)])
        self.assertEqual(rows[1]['color'], '#0f766e')

    def test_nav_lists_open_the_view_they_count(self):
        self.insert('task_lists', id='a', name='Alpha')
        inbox, alpha = self.query('nav_lists')
        self.assertEqual((inbox['filter'], inbox['list_id'], inbox['nav'], inbox['icon']), ('inbox', '', 'list-inbox', 'inbox'))
        self.assertEqual((alpha['filter'], alpha['list_id'], alpha['nav'], alpha['icon']), ('all', 'a', 'a', ''))

    def test_nav_counts_equal_rows_shown_with_subtasks(self):
        self.insert('task_lists', id='l', name='Work')
        self.add_task('p-inbox')
        self.add_task('s-inbox', parent_id='p-inbox')
        self.add_task('p-due', due='2026-11-01', list_id='l')
        self.add_task('s-due', due='2026-11-01', list_id='l', parent_id='p-due')
        self.add_task('p-later', due='2026-12-01')
        self.add_task('s-later', due='2026-12-02', parent_id='p-later')
        counts = self.query('nav_counts', self.TODAY)[0]
        self.assertEqual(counts['inbox'], 4)
        self.assertEqual(counts['upcoming'], 2)
        self.assertEqual(counts['today'], 2)
        self.assertEqual(counts['open'], 6)
        by_list = {r['id']: r['open'] for r in self.query('nav_lists')}
        self.assertEqual(by_list['inbox'], counts['inbox'])
        self.assertEqual(by_list['l'], 2)
        for key, filter in [('inbox', 'inbox'), ('today', 'today'), ('upcoming', 'upcoming'), ('open', 'all')]:
            self.assertEqual(counts[key], len(self.tree_rows(filter)), key)


if __name__ == '__main__':
    import unittest
    unittest.main()

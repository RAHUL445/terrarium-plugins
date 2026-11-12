"""Behavioral coverage for the workspace_tasks read model in the manifest."""
from organization_query import OrganizationQuery


class WorkspaceQuery(OrganizationQuery):
    TODAY = '2026-11-12'

    def workspace(self, *, filter='everything', q=None, tags=None, lists=None,
                  groups=None, statuses=None, tags_mode='any', category=None,
                  sort='manual', grouping='list'):
        return self.query('workspace_tasks', self.TODAY, filter, q, tags, lists,
                          groups, statuses, tags_mode, category, sort, grouping)

    def add_task(self, id, title=None, **values):
        self.insert('tasks', id=id, title=title or id, **values)

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

    def test_manual_title_and_priority_sorts(self):
        self.add_task('z', title='Zulu', priority=1, sort_order=1)
        self.add_task('a', title='Alpha', priority=3, sort_order=2)
        self.assertEqual([r['id'] for r in self.workspace(sort='manual')], ['z', 'a'])
        self.assertEqual([r['id'] for r in self.workspace(sort='title')], ['a', 'z'])
        self.assertEqual([r['id'] for r in self.workspace(sort='priority')], ['a', 'z'])


if __name__ == '__main__':
    import unittest
    unittest.main()

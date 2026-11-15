"""SQLite coverage for the versioned completed-task review query."""
from organization_query import OrganizationQuery, MANIFEST


class ReviewTasksQuery(OrganizationQuery):
    def review(self, start=None, end=None, page_size=100, offset=0,
               sort='completed_at', direction='desc'):
        return self.query('review_tasks', start, end, page_size, offset, sort, direction)

    def test_half_open_completion_range_raw_ms_and_stable_timestamp_ties(self):
        for values in [
            ('before', 999, 1, None, None),
            ('start', 1000, 1, None, None),
            ('cleared', 1250, 1, None, 4000),
            ('tie-b', 1500, 1, None, None),
            ('tie-a', 1500, 1, 'Outcome', None),
            ('end', 2000, 1, None, None),
            ('open', 1500, 0, None, None),
            ('trash', 1500, 1, None, None),
        ]:
            task_id, completed_at, done, done_note, cleared_at = values
            extras = {'_deleted': 1} if task_id == 'trash' else {}
            self.insert('tasks', id=task_id, title=task_id, completed_at=completed_at,
                        done=done, done_note=done_note, cleared_at=cleared_at, **extras)
        rows = self.review(1000, 2000, 100, 0, 'completed_at', 'asc')
        self.assertEqual([row['id'] for row in rows], ['start', 'cleared', 'tie-a', 'tie-b'])
        self.assertEqual(rows[0]['completed_at'], 1000)
        self.assertEqual(rows[0]['completed_at'], 1000)
        self.assertEqual(rows[2]['done_note'], 'Outcome')

    def test_pagination_reaches_tasks_beyond_legacy_500_limit(self):
        for index in range(625):
            self.insert('tasks', id=f'review-{index:04d}', title=f'Task {index:04d}',
                        done=1, completed_at=1500)
        page = self.review(1000, 2000, 101, 500, 'title', 'asc')
        tail = self.review(1000, 2000, 101, 600, 'title', 'asc')
        self.assertEqual(len(page), 101)
        self.assertEqual(page[0]['id'], 'review-0500')
        self.assertEqual(page[-1]['id'], 'review-0600')
        self.assertEqual([row['id'] for row in tail], [f'review-{i:04d}' for i in range(600, 625)])

    def test_review_view_declares_temporal_and_bounded_pagination(self):
        query = MANIFEST['queries']['review_tasks']
        self.assertEqual(query['view']['temporal'], {'start_param': 'start_ms', 'end_param': 'end_ms'})
        self.assertEqual(query['view']['pagination']['max_page_size'], 1000)
        for key in ('start_ms', 'end_ms', 'page_size', 'offset'):
            self.assertEqual(query['types'][key]['type'], 'integer')

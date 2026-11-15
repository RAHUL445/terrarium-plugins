"""Archive operations, run for real through `terra preview` (FIX-19).

Needs a built `dist/plugin.wasm` and a terra binary: set TERRA_BIN (for
example `$CARGO_TARGET_DIR/debug/terra`). Skipped otherwise."""
import json
import os
import socket
import subprocess
import tempfile
import time
import unittest
import urllib.error
import urllib.request
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1]
TERRA = os.environ.get('TERRA_BIN')
OFFSET = time.localtime().tm_gmtoff * 1000
DAY_MS = 86_400_000


class OpError(Exception):
    pass


@unittest.skipUnless(TERRA and (PACKAGE / 'dist' / 'plugin.wasm').exists(), 'set TERRA_BIN and build the plugin')
class ArchiveOps(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = tempfile.TemporaryDirectory(prefix='tasks-archive-')
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0))
            cls.port = s.getsockname()[1]
        cls.server = subprocess.Popen(
            [TERRA, 'preview', str(PACKAGE), '--port', str(cls.port), '--data', cls.data.name],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.time() + 60
        while True:
            try:
                cls.invoke('plugins', {})
                break
            except (urllib.error.URLError, ConnectionError):
                if time.time() > deadline or cls.server.poll() is not None:
                    cls.server.kill()
                    raise
                time.sleep(0.2)

    @classmethod
    def tearDownClass(cls):
        cls.server.terminate()
        cls.server.wait(10)
        cls.data.cleanup()

    @classmethod
    def invoke(cls, cmd, args):
        req = urllib.request.Request(f'http://127.0.0.1:{cls.port}/invoke',
                                     data=json.dumps({'cmd': cmd, 'args': args}).encode())
        try:
            with urllib.request.urlopen(req, timeout=30) as res:
                return json.loads(res.read())['result']
        except urllib.error.HTTPError as e:
            with e:
                raise OpError(json.loads(e.read())['error']) from None

    def setUp(self):
        # Every test starts with nothing waiting to be archived and auto-archive off.
        self.op('archive_settings', auto='never')
        self.op('clear_done')

    def op(self, _op, **args):
        return self.invoke('run_action', {'plugin': 'tasks', 'op': _op, 'args': args})

    def q(self, _query, **params):
        return self.invoke('run_query', {'plugin': 'tasks', 'name': _query, 'params': params})

    def uniq(self, name):
        self.__class__.n = getattr(self.__class__, 'n', 0) + 1
        return f'{name}-{self.__class__.n}'

    def new(self, title, done=False, **args):
        id = self.op('create', title=self.uniq(title), **args)['id']
        if done:
            self.op('toggle', id=id, done=True)
        return id

    def task(self, id):
        return self.q('task', id=id, now_ms=int(time.time() * 1000), utc_offset_ms=OFFSET)[0]

    def archived(self, **params):
        params.setdefault('today', time.strftime('%Y-%m-%d'))
        params.setdefault('now_ms', int(time.time() * 1000))
        params.setdefault('utc_offset_ms', OFFSET)
        return self.q('archived', **params)

    def archived_ids(self, **params):
        return {r['id'] for r in self.archived(**params)}

    def listed(self, include=None):
        params = {'today': time.strftime('%Y-%m-%d'), 'filter': 'everything'}
        if include is not None:
            params['include_archived'] = include
        return {r['id'] for r in self.q('tasks', **params)}

    def schedule(self):
        listing = self.invoke('command', {'kind': 'query', 'id': 'schedule.list', 'args': {}})
        # The plugin's storage id is `todo`.
        return [s for s in listing['schedules'] if s['plugin'] in ('tasks', 'todo') and s['key'] == 'auto-archive']

    # ---- archive / unarchive / undo ---------------------------------------------

    def test_archive_hides_a_done_task_keeps_it_done_and_unarchive_brings_it_back(self):
        t = self.new('Finished', done=True)
        self.assertEqual(self.task(t)['archived'], 0)
        self.assertEqual(self.task(t)['can_archive'], 1)
        made = self.op('archive', id=t)
        self.assertEqual((made['archived'], made['message']), (1, 'Archived'))
        self.assertGreater(made['at'], 0)
        row = self.task(t)
        self.assertEqual((row['archived'], row['done'], row['can_archive'], row['can_start_timer']), (1, 1, 0, 0))
        self.assertTrue(row['archived_label'].startswith('Archived '), row['archived_label'])
        self.assertNotIn(t, self.listed())
        self.assertIn(t, self.listed(include=True))
        self.assertIn(t, self.archived_ids())
        # Unarchiving again is the undo; the task is still done.
        back = self.op('unarchive', id=t)
        self.assertEqual((back['unarchived'], back['message']), (1, 'Unarchived'))
        row = self.task(t)
        self.assertEqual((row['archived'], row['done'], row['can_archive']), (0, 1, 1))
        self.assertIn(t, self.listed())
        self.assertNotIn(t, self.archived_ids())
        # The undo of an unarchive is an archive.
        self.assertEqual(self.op('archive', id=t)['archived'], 1)
        self.assertIn(t, self.archived_ids())

    def test_only_completed_live_tasks_can_be_archived(self):
        t = self.new('Still open')
        with self.assertRaisesRegex(OpError, 'Only completed tasks can be archived'):
            self.op('archive', id=t)
        self.assertEqual(self.task(t)['archived'], 0)
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('archive', id='missing')
        trashed = self.new('Trashed', done=True)
        self.op('delete', id=trashed)
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('archive', id=trashed)
        with self.assertRaisesRegex(OpError, 'Give a task'):
            self.op('unarchive')
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('unarchive', id='missing')
        with self.assertRaisesRegex(OpError, 'must be a number'):
            self.op('unarchive', at='soon')

    def test_archiving_twice_keeps_the_first_time_and_unarchiving_a_visible_task_is_harmless(self):
        t = self.new('Twice', done=True)
        first = self.op('archive', id=t)
        again = self.op('archive', id=t)
        self.assertEqual((again['archived'], again['at'], again['message']), (0, first['at'], 'Already archived'))
        self.op('unarchive', id=t)
        none = self.op('unarchive', id=t)
        self.assertEqual((none['unarchived'], none['message']), (0, 'Nothing to unarchive'))
        self.assertEqual(self.task(t)['done'], 1)

    def test_reopening_an_archived_task_brings_it_back_to_the_lists(self):
        t = self.new('Reopened', done=True)
        self.op('archive', id=t)
        self.op('toggle', id=t, done=False)
        row = self.task(t)
        self.assertEqual((row['archived'], row['done']), (0, 0))
        self.assertIn(t, self.listed())

    def test_trashing_an_archived_task_and_restoring_it_keeps_it_archived(self):
        t = self.new('Trash me', done=True)
        self.op('archive', id=t)
        self.op('delete', id=t)
        self.assertNotIn(t, self.archived_ids())
        self.op('restore', id=t)
        self.assertIn(t, self.archived_ids())
        self.assertNotIn(t, self.listed())

    # ---- subtasks -------------------------------------------------------------------

    def test_a_parent_archives_with_its_done_subtasks_and_open_ones_stay(self):
        parent = self.new('Parent')
        done_child = self.new('Done child', parent_id=parent)
        done_child2 = self.new('Done child 2', parent_id=parent)
        open_child = self.new('Open child', parent_id=parent)
        for c in (done_child, done_child2):
            self.op('toggle', id=c, done=True)
        self.op('toggle', id=parent, done=True)
        made = self.op('archive', id=parent)
        self.assertEqual((made['archived'], made['message']), (3, 'Archived with 2 subtasks'))
        self.assertEqual(self.archived_ids() & {parent, done_child, done_child2, open_child},
                         {parent, done_child, done_child2})
        self.assertEqual(self.task(open_child)['archived'], 0)
        # The open subtask shows as a top-level task while its parent is archived.
        rows = {r['id']: r for r in self.q('tasks', today=time.strftime('%Y-%m-%d'), filter='everything')}
        self.assertIn(open_child, rows)
        self.assertIsNone(rows[open_child]['parent_label'])
        # Undo (unarchive the parent) takes the done subtasks back with it.
        back = self.op('unarchive', id=parent)
        self.assertEqual((back['unarchived'], back['message']), (3, 'Unarchived 3 tasks'))
        self.assertEqual(self.archived_ids() & {parent, done_child, done_child2, open_child}, set())
        self.assertTrue({parent, done_child, done_child2, open_child} <= self.listed())

    def test_a_subtask_archived_on_its_own_stays_archived_when_the_parent_comes_back(self):
        parent = self.new('Parent two')
        child = self.new('Child two', parent_id=parent)
        self.op('toggle', id=child, done=True)
        self.op('toggle', id=parent, done=True)
        self.op('archive', id=child)
        time.sleep(0.01)
        one = self.op('archive', id=parent)
        self.assertEqual((one['archived'], one['message']), (1, 'Archived'))
        self.op('unarchive', id=parent)
        self.assertEqual((self.task(parent)['archived'], self.task(child)['archived']), (0, 1))

    def test_a_single_done_task_archives_with_one_subtask_wording(self):
        parent = self.new('One child parent')
        child = self.new('Only child', parent_id=parent, )
        self.op('toggle', id=child, done=True)
        self.op('toggle', id=parent, done=True)
        self.assertEqual(self.op('archive', id=parent)['message'], 'Archived with 1 subtask')

    # ---- Archive completed / unarchive {at} ------------------------------------------------

    def test_archive_completed_archives_everything_done_and_at_undoes_exactly_that_batch(self):
        old = self.new('Archived earlier', done=True)
        self.op('archive', id=old)
        time.sleep(0.01)
        a, b = self.new('Batch a', done=True), self.new('Batch b', done=True)
        still_open = self.new('Not done')
        made = self.op('clear_done')
        self.assertEqual((made['cleared'], made['archived']), (2, 2))
        self.assertGreater(made['at'], 0)
        self.assertEqual(self.archived_ids() & {old, a, b, still_open}, {old, a, b})
        undone = self.op('unarchive', at=made['at'])
        self.assertEqual((undone['unarchived'], undone['message']), (2, 'Unarchived 2 tasks'))
        self.assertEqual(self.archived_ids() & {old, a, b, still_open}, {old})
        self.assertTrue({a, b, still_open} <= self.listed())
        # Both done tasks are still done. at also arrives as text.
        self.assertEqual((self.task(a)['done'], self.task(b)['done']), (1, 1))
        again = self.op('archive_done')
        self.assertEqual(again['archived'], 2)
        self.assertEqual(self.op('unarchive', at=str(again['at']))['unarchived'], 2)

    def test_archive_completed_with_nothing_done_is_a_no_op(self):
        made = self.op('clear_done')
        self.assertEqual((made['cleared'], made['archived']), (0, 0))
        self.assertEqual(self.op('unarchive', at=made['at'])['unarchived'], 0)

    def test_each_repeat_occurrence_is_its_own_row_and_archives_separately(self):
        first = self.new('Water plants', repeat='weekly')
        second = self.op('toggle', id=first, done=True)['next']['id']
        self.assertNotEqual(first, second)
        self.op('archive', id=first)
        # The next occurrence is open and stays in the lists.
        self.assertIn(second, self.listed())
        self.assertIn(first, self.archived_ids())
        self.assertNotIn(second, self.archived_ids())
        third = self.op('toggle', id=second, done=True)['next']['id']
        self.op('archive', id=second)
        rows = {r['id']: r for r in self.archived()}
        self.assertEqual({first, second} <= set(rows), True)
        self.assertEqual((rows[first]['repeat_label'], rows[second]['repeat_label']), ('Weekly', 'Weekly'))
        self.assertNotIn(third, rows)
        # Unarchiving one occurrence leaves the other archived.
        self.op('unarchive', id=first)
        self.assertEqual((self.task(first)['archived'], self.task(second)['archived']), (0, 1))

    # ---- who still sees archived tasks -------------------------------------------------------

    def test_lists_and_search_skip_archived_tasks_but_reports_still_count_them(self):
        title = self.uniq('Searchable archived')
        t = self.op('create', title=title)['id']
        self.op('toggle', id=t, done=True)
        self.op('add_time', task_id=t, minutes=25)
        self.op('archive', id=t)
        self.assertEqual([r['id'] for r in self.q('search', q=title)], [])
        self.assertNotIn(t, self.listed())
        self.assertIn(t, self.listed(include=True))
        self.assertNotIn(t, {r['id'] for r in self.q('tasks', today=time.strftime('%Y-%m-%d'), filter='done',
                                                      include_archived=False)})
        today = time.strftime('%Y-%m-%d')
        between = self.op('completed_between', start=today, end=today)['tasks']
        self.assertIn(t, {r['id'] for r in between})
        summary = self.q('time_summary', **{'from': today, 'to': today, 'by': 'task',
                                              'now_ms': int(time.time() * 1000), 'utc_offset_ms': OFFSET})
        self.assertIn((t, 25), [(r['id'], r['minutes']) for r in summary])
        now = int(time.time() * 1000)
        review = self.q('review_tasks', start_ms=now - DAY_MS, end_ms=now + DAY_MS, page_size=500, offset=0,
                        sort='completed_at', direction='desc')
        self.assertIn(t, {r['id'] for r in review})

    def test_the_archive_query_searches_filters_and_groups_real_tasks(self):
        today = time.strftime('%Y-%m-%d')
        t = self.new('Finished for the archive', done=True)
        self.op('save_note', id=t, text='Shipped it\nand wrote it up')
        self.op('archive', id=t)
        rows = {r['id']: r for r in self.archived()}
        self.assertEqual(rows[t]['group_label'], 'Today')
        self.assertEqual((rows[t]['done_line'], rows[t]['completed_date']), ('Shipped it', today))
        self.assertEqual(rows[t]['list_name'], 'Inbox')
        self.assertEqual(self.archived_ids(q='wrote it up') & {t}, {t})
        self.assertEqual(self.archived_ids(q='zzzz-not-there'), set())
        self.assertEqual(self.archived_ids(**{'from': today, 'to': today}) & {t}, {t})
        self.assertEqual(self.archived_ids(**{'from': '2020-01-01', 'to': '2020-01-31'}), set())
        self.assertEqual(self.archived_ids(list_id='@inbox') & {t}, {t})
        stats = self.q('archive_stats', today=today, utc_offset_ms=OFFSET)[0]
        self.assertGreaterEqual(stats['done_month'], 1)
        self.assertIn('done this month', stats['label'])

    # ---- settings and auto-archive ---------------------------------------------------------------

    def run_auto(self, slot_ms, date=None):
        run = {'slot_ms': slot_ms, 'id': f'test-{slot_ms}', 'schedule': 'auto-archive'}
        if date:
            run['date'] = date
        return self.op('auto_archive', **{'$run': run})

    def test_settings_validate_save_and_toggle_the_schedule(self):
        self.assertEqual(self.q('archive_settings'), [{'auto': 'never'}])
        for bad in ['', 'weekly', '7', 'Never']:
            with self.assertRaisesRegex(OpError, 'auto must be'):
                self.op('archive_settings', auto=bad)
        self.assertEqual(self.schedule(), [])
        saved = self.op('archive_settings', auto='7d')
        self.assertEqual((saved['auto'], saved['archived']), ('7d', 0))
        self.assertIn('after 7 days', saved['message'])
        self.assertEqual(self.q('archive_settings'), [{'auto': '7d'}])
        schedule = self.schedule()
        self.assertEqual(len(schedule), 1, schedule)
        self.assertEqual(schedule[0]['name'], 'Auto-archive completed tasks')
        self.assertEqual(schedule[0]['operation'], 'auto_archive')
        self.assertEqual(schedule[0]['repeat'], {'every': 'day', 'at': '00:05'})
        # Changing the mode keeps one schedule; turning it off removes it.
        self.op('archive_settings', auto='end_of_day')
        self.assertEqual(len(self.schedule()), 1)
        self.assertEqual(self.q('archive_settings'), [{'auto': 'end_of_day'}])
        off = self.op('archive_settings', auto='never')
        self.assertEqual(off['message'], 'Auto-archive is off')
        self.assertEqual(self.schedule(), [])
        self.assertEqual(self.q('archive_settings'), [{'auto': 'never'}])
        # Turning it off again, with no schedule, is fine.
        self.op('archive_settings', auto='never')

    def test_settings_share_one_row_with_the_note_settings(self):
        self.op('note_settings', ask='never')
        self.op('archive_settings', auto='30d')
        self.assertEqual(self.q('archive_settings'), [{'auto': '30d'}])
        self.assertEqual(self.q('note_settings')[0]['ask'], 'never')
        self.op('note_settings', ask='tagged', tags='work')
        self.assertEqual(self.q('archive_settings'), [{'auto': '30d'}])
        self.assertEqual(self.q('note_settings')[0]['ask'], 'tagged')

    def test_auto_archive_does_nothing_when_never(self):
        t = self.new('Never archived', done=True)
        far = int(time.time() * 1000) + 400 * DAY_MS
        result = self.run_auto(far, time.strftime('%Y-%m-%d'))
        self.assertEqual(result, {'archived': 0})
        self.assertEqual(self.task(t)['archived'], 0)
        self.assertEqual(self.op('auto_archive'), {'archived': 0})

    def test_auto_archive_after_7_and_30_days(self):
        now = int(time.time() * 1000)
        today = time.strftime('%Y-%m-%d')
        a, b = self.new('Done a', done=True), self.new('Done b', done=True)
        open_task = self.new('Never done')
        self.op('archive_settings', auto='7d')
        self.assertEqual(self.run_auto(now + 6 * DAY_MS, today), {'archived': 0})
        self.assertEqual(self.archived_ids() & {a, b}, set())
        hit = self.run_auto(now + 8 * DAY_MS, today)
        self.assertEqual(hit['archived'] >= 2, True)
        self.assertEqual(hit['at'], now + 8 * DAY_MS)  # the slot is the archive time
        self.assertEqual(hit['$run']['notify'], f"Archived {hit['archived']} completed tasks.")
        self.assertEqual(self.archived_ids() & {a, b, open_task}, {a, b})
        # Repeating the same slot (a second device) changes nothing and stays quiet.
        again = self.run_auto(now + 8 * DAY_MS, today)
        self.assertEqual(again, {'archived': 0})
        # One undo by the slot time brings the batch back.
        self.assertEqual(self.op('unarchive', at=hit['at'])['unarchived'], hit['archived'])
        # 30 days: a week is not enough, a month and a bit is.
        self.op('archive_settings', auto='30d')
        self.assertEqual(self.run_auto(now + 29 * DAY_MS, today), {'archived': 0})
        self.assertEqual(self.run_auto(now + 31 * DAY_MS, today)['archived'] >= 2, True)
        self.assertEqual(self.archived_ids() & {a, b}, {a, b})
        self.assertEqual(self.task(open_task)['archived'], 0)

    def test_auto_archive_at_the_end_of_the_day(self):
        now = int(time.time() * 1000)
        today = time.strftime('%Y-%m-%d')
        tomorrow = time.strftime('%Y-%m-%d', time.localtime(time.time() + DAY_MS / 1000))
        t = self.new('Finished today', done=True)
        self.op('archive_settings', auto='end_of_day')
        # Finished today, so tonight's pass leaves it for tomorrow's.
        self.assertEqual(self.run_auto(now, today), {'archived': 0})
        self.assertEqual(self.task(t)['archived'], 0)
        slot = now + DAY_MS
        hit = self.run_auto(slot, tomorrow)
        self.assertGreaterEqual(hit['archived'], 1)
        self.assertEqual(self.task(t)['archived'], 1)
        self.assertEqual(self.run_auto(slot, tomorrow), {'archived': 0})

    def test_auto_archive_leaves_already_archived_tasks_alone(self):
        now = int(time.time() * 1000)
        t = self.new('Archived by hand', done=True)
        made = self.op('archive', id=t)
        self.op('archive_settings', auto='7d')
        self.run_auto(now + 10 * DAY_MS, time.strftime('%Y-%m-%d'))
        # Its archive time is still the one it got by hand: the batch undo finds it.
        self.assertEqual(self.op('unarchive', at=made['at'])['unarchived'], 1)
        self.assertEqual(self.task(t)['archived'], 0)


if __name__ == '__main__':
    unittest.main()

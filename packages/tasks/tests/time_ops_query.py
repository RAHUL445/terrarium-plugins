"""Time tracking operations, run for real through `terra preview` (FIX-18).

Needs a built `dist/plugin.wasm` and a terra binary: set TERRA_BIN (for
example `$CARGO_TARGET_DIR/debug/terra`). Skipped otherwise."""
import csv
import io
import json
import os
import socket
import subprocess
import tempfile
import time
import unittest
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1]
TERRA = os.environ.get('TERRA_BIN')
OFFSET = time.localtime().tm_gmtoff * 1000


class OpError(Exception):
    pass


@unittest.skipUnless(TERRA and (PACKAGE / 'dist' / 'plugin.wasm').exists(), 'set TERRA_BIN and build the plugin')
class TimeOps(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = tempfile.TemporaryDirectory(prefix='tasks-time-')
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
        # One timer per space: start each test with none running.
        self.op('stop_timer')

    def op(self, _op, **args):
        return self.invoke('run_action', {'plugin': 'tasks', 'op': _op, 'args': args})

    def q(self, _query, **params):
        return self.invoke('run_query', {'plugin': 'tasks', 'name': _query, 'params': params})

    def new(self, title, **args):
        return self.op('create', title=title, **args)['id']

    def task(self, id, now=None):
        now = now or int(time.time() * 1000)
        return self.q('task', id=id, now_ms=now, utc_offset_ms=OFFSET)[0]

    def entries(self, task, now=None):
        now = now or int(time.time() * 1000)
        return self.q('time_entries', task_id=task, now_ms=now, utc_offset_ms=OFFSET, today=time.strftime('%Y-%m-%d'))

    def running(self, now=None):
        return self.q('running_timer', now_ms=now or int(time.time() * 1000), utc_offset_ms=OFFSET)

    def estimate(self, id):
        return self.task(id)['own_estimate_min']

    def uniq(self, name):
        self.__class__.n = getattr(self.__class__, 'n', 0) + 1
        return f'{name}-{self.__class__.n}'

    # ---- estimate and duration parsing --------------------------------------

    def test_set_estimate_reads_every_format_and_clears(self):
        t = self.new(self.uniq('Estimated'))
        for text, minutes in [('1h 30m', 90), ('90m', 90), ('1.5h', 90), ('45', 45), (' 2H ', 120), ('1:30', 90),
                              ('2 hours 15 minutes', 135), (75, 75)]:
            self.op('set_estimate', id=t, minutes=text)
            self.assertEqual(self.estimate(t), minutes, text)
        self.assertEqual(self.task(t)['estimate_text'], '1h 15m')
        done = self.op('set_estimate', id=t, minutes='3h')
        self.assertEqual((done['estimate_min'], done['previous'], done['changed']), (180, 75, True))
        same = self.op('set_estimate', id=t, minutes='180')
        self.assertEqual((same['changed'], same['previous']), (False, 180))
        # Undo = the previous value back; empty, null and 0 clear.
        self.op('set_estimate', id=t, minutes=done['previous'])
        self.assertEqual(self.estimate(t), 75)
        cleared = self.op('set_estimate', id=t, minutes='')
        self.assertEqual((cleared['estimate_min'], cleared['previous']), (None, 75))
        self.assertIsNone(self.estimate(t))
        self.assertEqual(self.op('set_estimate', id=t, minutes=cleared['previous'])['estimate_min'], 75)
        for clear in (None, 0, '  '):
            self.op('set_estimate', id=t, minutes=clear)
            self.assertIsNone(self.estimate(t))
        self.assertEqual(self.op('set_estimate', id=t)['changed'], False)

    def test_garbage_durations_are_refused_with_a_clear_message(self):
        t = self.new(self.uniq('Strict'))
        for bad in ['soon', '1x', '1h 1h', '-5', '1:75', '1001h', 1.5, True, '1.5']:
            with self.assertRaisesRegex(OpError, r'Couldn.t read|whole number|at least|limited|must be a number'):
                self.op('set_estimate', id=t, minutes=bad)
            self.assertIsNone(self.estimate(t))
        with self.assertRaisesRegex(OpError, 'Try 1h 30m'):
            self.op('add_time', task_id=t, duration='lots')
        with self.assertRaisesRegex(OpError, 'Give the time spent'):
            self.op('add_time', task_id=t)
        with self.assertRaisesRegex(OpError, 'at least 1 minute'):
            self.op('add_time', task_id=t, minutes=0)
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('set_estimate', id='missing', minutes=5)
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('add_time', task_id='missing', minutes=5)
        self.assertEqual(self.entries(t), [])

    # ---- manual time ------------------------------------------------------

    def test_add_time_with_date_and_note_then_edit_delete_restore(self):
        t = self.new(self.uniq('Logged'))
        made = self.op('add_time', task_id=t, duration='1h 30m', date='2026-11-10', note='  Drafted the outline ')
        self.assertEqual((made['minutes'], made['duration'], made['message']), (90, '1h 30m', 'Added 1h 30m'))
        now = self.op('add_time', task_id=t, minutes=25)
        rows = {r['id']: r for r in self.entries(t)}
        self.assertEqual((rows[made['id']]['date'], rows[made['id']]['note'], rows[made['id']]['minutes']),
                         ('2026-11-10', 'Drafted the outline', 90))
        self.assertEqual(rows[now['id']]['date'], time.strftime('%Y-%m-%d'))
        self.assertEqual([r['id'] for r in self.entries(t)], [now['id'], made['id']])  # newest first
        self.assertEqual(self.task(t)['tracked_min'], 115)
        with self.assertRaisesRegex(OpError, 'date must look like'):
            self.op('add_time', task_id=t, minutes=5, date='yesterday')
        with self.assertRaisesRegex(OpError, 'limited to 500'):
            self.op('add_time', task_id=t, minutes=5, note='x' * 501)
        # Edit: time, date, note; the previous values come back for Undo.
        edit = self.op('update_time_entry', id=made['id'], duration='2h', date='2026-11-09', note='')
        self.assertEqual((edit['previous_minutes'], edit['previous_date'], edit['previous_note']),
                         (90, '2026-11-10', 'Drafted the outline'))
        after = {r['id']: r for r in self.entries(t)}[made['id']]
        self.assertEqual((after['minutes'], after['date'], after['note']), (120, '2026-11-09', None))
        self.op('update_time_entry', id=made['id'], minutes=edit['previous_minutes'], date=edit['previous_date'],
                note=edit['previous_note'])
        back = {r['id']: r for r in self.entries(t)}[made['id']]
        self.assertEqual((back['minutes'], back['date'], back['note']), (90, '2026-11-10', 'Drafted the outline'))
        with self.assertRaisesRegex(OpError, 'nothing to update'):
            self.op('update_time_entry', id=made['id'])
        # Delete goes to the trash; restore brings it back; both are idempotent.
        gone = self.op('delete_time_entry', id=made['id'])
        self.assertEqual((gone['deleted'], gone['minutes']), (1, 90))
        self.assertEqual(self.task(t)['tracked_min'], 25)
        self.assertEqual(self.op('delete_time_entry', id=made['id'])['deleted'], 0)
        self.assertEqual(self.op('restore_time_entry', id=made['id'])['restored'], 1)
        self.assertEqual(self.op('restore_time_entry', id=made['id'])['restored'], 0)
        self.assertEqual(self.task(t)['tracked_min'], 115)
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('restore_time_entry', id='missing')

    # ---- timers -----------------------------------------------------------

    def test_one_running_timer_per_space_starting_stops_the_other(self):
        a, b = self.new(self.uniq('A')), self.new(self.uniq('B'))
        first = self.op('start_timer', task_id=a)
        self.assertEqual((first['already_running'], first['stopped']), (False, []))
        self.assertEqual(first['message'], 'Timer started')
        self.assertEqual([r['task_id'] for r in self.running()], [a])
        second = self.op('start_timer', task_id=b)
        self.assertEqual([s['task_id'] for s in second['stopped']], [a])
        self.assertEqual(second['stopped'][0]['minutes'], 1)  # stopped at once: minimum one minute
        self.assertEqual(second['stopped_ids'], first['id'])
        self.assertIn('stopped', second['message'])
        running = self.running()
        self.assertEqual([r['task_id'] for r in running], [b])
        self.assertEqual(running[0]['running_count'], 1)
        old = self.entries(a)[0]
        self.assertEqual((old['running'], old['minutes']), (0, 1))
        # Starting the task that already runs changes nothing.
        again = self.op('start_timer', task_id=b)
        self.assertEqual((again['already_running'], again['id']), (True, second['id']))
        self.assertEqual(len(self.entries(b)), 1)
        # Undo of the second start: its entry goes away, the first runs again.
        self.op('undo_start_timer', id=second['id'], stopped_ids=second['stopped_ids'])
        self.assertEqual([r['task_id'] for r in self.running()], [a])
        self.assertEqual(self.entries(b), [])
        self.assertEqual(self.entries(a)[0]['running'], 1)

    def test_stop_timer_rounds_up_to_one_minute_and_resume_undoes_it(self):
        t = self.new(self.uniq('Short'))
        started = self.op('start_timer', task_id=t)
        stopped = self.op('stop_timer')
        self.assertEqual((stopped['stopped'], stopped['minutes'], stopped['task_id']), (1, 1, t))
        self.assertEqual(stopped['ids'], started['id'])
        self.assertIn('Timer stopped · 1m', stopped['message'])
        self.assertEqual(self.running(), [])
        self.assertEqual(self.task(t)['tracked_min'], 1)
        # Nothing running is not an error.
        nothing = self.op('stop_timer')
        self.assertEqual((nothing['stopped'], nothing['ids']), (0, ''))
        # Undo = resume: the entry runs again and its time is replaced by elapsed time.
        self.op('resume_timer', ids=stopped['ids'])
        self.assertEqual([r['task_id'] for r in self.running()], [t])
        self.assertEqual(self.entries(t)[0]['running'], 1)
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('resume_timer', ids='missing')
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('start_timer', task_id='missing')

    def test_running_elapsed_is_in_the_totals_and_rows(self):
        t = self.new(self.uniq('Live'), tags='timer')
        self.op('set_estimate', id=t, minutes='1h')
        self.op('add_time', task_id=t, minutes=20)
        started = self.op('start_timer', task_id=t)
        begin = self.running()[0]['started_at']
        ten_later = (begin + 10 * 60) * 1000
        row = self.task(t, ten_later)
        self.assertEqual((row['running'], row['tracked_min'], row['time_label']), (1, 30, '30m / 1h'))
        self.assertEqual(row['running_label'], '● Running')
        self.assertIn('10m so far', row['timer_note'])
        ws = next(r for r in self.q('workspace_tasks', today=time.strftime('%Y-%m-%d'), filter='everything', now=ten_later)
                  if r['id'] == t)
        self.assertEqual((ws['tracked_min'], ws['running'], ws['time_label'], ws['running_label']),
                         (30, 1, '30m / 1h', '● Running'))
        live = self.entries(t, ten_later)[0]
        self.assertEqual((live['id'], live['running'], live['minutes'], live['duration_label']),
                         (started['id'], 1, 10, '10m · running'))
        self.assertEqual(self.running(ten_later)[0]['elapsed_label'], '10m')
        # An hour and a half in: over the estimate, shown in the danger tone.
        over = self.task(t, (begin + 90 * 60) * 1000)
        self.assertEqual((over['tracked_min'], over['time_tone'], over['time_over']), (110, 'danger', 'Over by 50m'))

    def test_stop_stops_every_running_row_whichever_device_started_it(self):
        # Two devices starting a timer offline leave two running rows. Restoring a
        # deleted running entry makes the same state here.
        a, b = self.new(self.uniq('A')), self.new(self.uniq('B'))
        first = self.op('start_timer', task_id=a)['id']
        self.op('delete_time_entry', id=first)
        self.op('start_timer', task_id=b)
        self.op('restore_time_entry', id=first)
        running = self.running()
        self.assertEqual(running[0]['running_count'], 2)
        stopped = self.op('stop_timer')
        self.assertEqual((stopped['stopped'], stopped['minutes']), (2, 2))
        self.assertEqual(sorted(stopped['ids'].split(',')), sorted([first, self.entries(b)[0]['id']]))
        self.assertEqual(self.running(), [])
        # Starting one with a duplicate pair running stops both others first.
        self.op('resume_timer', ids=first)
        self.op('start_timer', task_id=b)
        self.assertEqual([r['task_id'] for r in self.running()], [b])

    def test_a_running_entry_can_only_get_a_note(self):
        t = self.new(self.uniq('Noted'))
        entry = self.op('start_timer', task_id=t)['id']
        with self.assertRaisesRegex(OpError, 'Stop the timer'):
            self.op('update_time_entry', id=entry, minutes=30)
        with self.assertRaisesRegex(OpError, 'Stop the timer'):
            self.op('update_time_entry', id=entry, date='2026-11-01')
        self.op('update_time_entry', id=entry, note='Pairing')
        self.assertEqual(self.entries(t)[0]['note'], 'Pairing')
        self.assertEqual(self.entries(t)[0]['running'], 1)
        # Deleting a running entry ends the timer with it.
        self.op('delete_time_entry', id=entry)
        self.assertEqual(self.running(), [])

    # ---- tasks that finish or go away ---------------------------------------

    def test_finishing_a_task_stops_its_timer_and_a_done_task_cannot_start_one(self):
        t = self.new(self.uniq('Finish me'))
        self.op('start_timer', task_id=t)
        result = self.op('toggle', id=t, done=True)
        self.assertIn('Task done', result['message'])
        self.assertIn('timer stopped at 1m', result['message'])
        self.assertEqual(self.running(), [])
        self.assertEqual(self.task(t)['tracked_min'], 1)
        with self.assertRaisesRegex(OpError, 'is done'):
            self.op('start_timer', task_id=t)
        quiet = self.op('toggle', id=t, done=False)
        self.assertEqual(quiet['message'], 'Task reopened')
        self.op('start_timer', task_id=t)
        # Another task finishing leaves this timer alone.
        other = self.new(self.uniq('Other'))
        self.assertEqual(self.op('toggle', id=other, done=True)['message'], 'Task done')
        self.assertEqual([r['task_id'] for r in self.running()], [t])
        # The board lane "done" goes through toggle too.
        self.op('move', id=t, lane='done')
        self.assertEqual(self.running(), [])

    def test_repeating_task_keeps_its_estimate_but_not_its_time(self):
        t = self.new(self.uniq('Weekly'), repeat='weekly')
        self.op('set_estimate', id=t, minutes='45m')
        self.op('add_time', task_id=t, minutes=30)
        nxt = self.op('toggle', id=t, done=True)['next']['id']
        self.assertEqual(self.estimate(nxt), 45)
        self.assertEqual(self.task(nxt)['tracked_min'], 0)

    def test_trashing_a_task_stops_its_timer_and_restore_brings_the_time_back(self):
        parent = self.new(self.uniq('Parent'))
        child = self.new(self.uniq('Child'), parent_id=parent)
        self.op('add_time', task_id=child, minutes=15)
        self.op('start_timer', task_id=child)
        self.op('delete', id=parent)
        self.assertEqual(self.running(), [])
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('start_timer', task_id=child)
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('add_time', task_id=child, minutes=5)
        self.op('restore', id=parent)
        self.assertEqual(self.task(child)['tracked_min'], 16)
        self.assertEqual(self.task(parent)['tracked_min'], 16)
        self.assertEqual(self.running(), [])  # not resumed

    # ---- rollup -------------------------------------------------------------

    def test_parent_rolls_up_subtasks_in_task_and_rows(self):
        parent = self.new(self.uniq('Launch'))
        c1 = self.new(self.uniq('Draft'), parent_id=parent)
        c2 = self.new(self.uniq('Review'), parent_id=parent)
        self.op('set_estimate', id=parent, minutes='1h')
        self.op('set_estimate', id=c1, minutes='1h 30m')
        self.op('add_time', task_id=parent, minutes=30)
        self.op('add_time', task_id=c1, minutes=45)
        self.op('add_time', task_id=c2, minutes=15)
        row = self.task(parent)
        self.assertEqual((row['own_estimate_min'], row['estimate_min']), (60, 150))
        self.assertEqual((row['own_tracked_min'], row['tracked_min']), (30, 90))
        self.assertEqual(row['time_label'], '1h 30m / 2h 30m')
        self.assertEqual(row['time_subtasks'], 'Includes subtasks: 1h of 1h 30m')
        child = self.task(c1)
        self.assertEqual((child['estimate_min'], child['tracked_min'], child['time_label']), (90, 45, '45m / 1h 30m'))
        today = time.strftime('%Y-%m-%d')
        rows = {r['id']: r for r in self.q('workspace_tasks', today=today, filter='everything')}
        self.assertEqual(rows[parent]['time_label'], '1h 30m / 2h 30m')
        self.assertNotIn(c1, rows)
        # A subtask's running timer shows on its parent and counts for it.
        self.op('start_timer', task_id=c2)
        two_later = (self.running()[0]['started_at'] + 120) * 1000
        row = self.task(parent, two_later)
        self.assertEqual((row['running'], row['running_sub'], row['running_label']), (0, 1, '● Running'))
        self.assertEqual(row['tracked_min'], 92)  # the subtask's two running minutes count for the parent
        self.assertIsNone(row['timer_note'])  # its own timer isn't running
        listed = {r['id']: r for r in self.q('workspace_tasks', today=today, filter='everything', now=two_later)}
        self.assertEqual((listed[parent]['running_label'], listed[parent]['running'], listed[parent]['tracked_min']),
                         ('● Running', 0, 92))
        # A trashed subtask leaves the rollup, and restoring puts it back.
        self.op('stop_timer')
        self.op('delete', id=c1)
        self.assertEqual((self.task(parent)['estimate_min'], self.task(parent)['tracked_min']), (60, 46))
        self.op('restore', id=c1)
        self.assertEqual((self.task(parent)['estimate_min'], self.task(parent)['tracked_min']), (150, 91))
        self.assertEqual(self.running(), [])

    # ---- summary and sort -----------------------------------------------------

    def test_time_summary_by_task_and_list_and_sort(self):
        work = self.op('create_list', name=self.uniq('Work'))['id']
        a = self.new(self.uniq('Alpha'), list_id=work)
        b = self.new(self.uniq('Beta'), list_id=work)
        inbox = self.new(self.uniq('Loose'))
        self.op('add_time', task_id=a, minutes=60, date='2020-01-02')
        self.op('add_time', task_id=a, minutes=30, date='2020-01-03')
        self.op('add_time', task_id=b, minutes=15, date='2020-01-03')
        self.op('add_time', task_id=inbox, minutes=45, date='2019-12-31')
        params = dict(now_ms=int(time.time() * 1000), utc_offset_ms=OFFSET)
        by_task = self.q('time_summary', **{'from': '2020-01-01', 'to': '2020-01-31', 'by': 'task'}, **params)
        self.assertEqual([(r['minutes'], r['entries']) for r in by_task], [(90, 2), (15, 1)])
        self.assertEqual([r['id'] for r in by_task], [a, b])
        by_list = self.q('time_summary', **{'from': '2019-12-01', 'to': '2020-01-31', 'by': 'list'}, **params)
        self.assertEqual([(r['title'], r['minutes'], r['tasks']) for r in by_list][0][1:], (105, 2))
        self.assertIn(('Inbox', 45, 1), [(r['title'], r['minutes'], r['tasks']) for r in by_list])
        self.assertEqual(self.q('time_summary', **{'from': '2020-01-03', 'to': '2020-01-03'}, **params)[0]['minutes'], 30)
        # Sort by time tracked or estimate (what the Sort menu sends).
        self.op('set_estimate', id=b, minutes='5h')
        today = time.strftime('%Y-%m-%d')
        ids = lambda sort: [r['id'] for r in self.q('workspace_tasks', today=today, filter='all', sort=sort, lists=work)]
        self.assertEqual(ids('tracked'), [a, b])
        self.assertEqual(ids('estimate'), [b, a])

    # ---- export ---------------------------------------------------------------

    def test_space_export_carries_entries_and_the_csv_columns(self):
        t = self.new(self.uniq('Export time'))
        self.op('set_estimate', id=t, minutes='1h 30m')
        entry = self.op('add_time', task_id=t, minutes=45, note='Marker note')['id']
        path = Path(self.data.name) / f'export-{time.time_ns()}.zip'
        self.invoke('command', {'kind': 'action', 'id': 'space.export', 'args': {'path': str(path)}})
        with zipfile.ZipFile(path) as z:
            names = z.namelist()
            blob = '\n'.join(z.read(n).decode('utf-8', 'replace') for n in names if n.endswith('.json'))
            self.assertIn('time_entries', blob)
            self.assertIn(entry, blob)
            self.assertIn('Marker note', blob)
            csv_name = next(n for n in names if n.endswith('tasks.csv'))
            rows = list(csv.DictReader(io.StringIO(z.read(csv_name).decode())))
        mine = next(r for r in rows if r['title'].startswith('Export time'))
        self.assertEqual((mine['estimate'], mine['tracked']), ('1h 30m', '45m'))


if __name__ == '__main__':
    unittest.main()

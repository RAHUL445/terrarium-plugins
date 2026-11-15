"""Subtask and dependency operations, run for real through `terra preview`.

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


class OpError(Exception):
    pass


@unittest.skipUnless(TERRA and (PACKAGE / 'dist' / 'plugin.wasm').exists(), 'set TERRA_BIN and build the plugin')
class StructureOps(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = tempfile.TemporaryDirectory(prefix='tasks-ops-')
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

    def op(self, _op, **args):
        return self.invoke('run_action', {'plugin': 'tasks', 'op': _op, 'args': args})

    def q(self, _query, **params):
        return self.invoke('run_query', {'plugin': 'tasks', 'name': _query, 'params': params})

    def task(self, id):
        rows = self.q('task', id=id)
        return rows[0] if rows else None

    def new(self, title, **args):
        return self.op('create', title=title, **args)['id']

    def test_one_level_rule_and_inheritance(self):
        work = self.op('create_list', name='Work')['id']
        parent = self.new('Launch', list_id=work)
        child = self.new('Draft', parent_id=parent)
        self.assertEqual((self.task(child)['parent_id'], self.task(child)['list_id']), (parent, work))
        with self.assertRaisesRegex(OpError, 'one level'):
            self.new('Too deep', parent_id=child)
        other = self.new('Other')
        with self.assertRaisesRegex(OpError, 'one level'):
            self.op('set_parent', id=parent, parent_id=other)
        with self.assertRaisesRegex(OpError, 'own subtask'):
            self.op('set_parent', id=other, parent_id=other)
        # Moving under a parent joins its list; an empty parent detaches.
        moved = self.op('set_parent', id=other, parent_id=parent)
        self.assertEqual(moved['previous_parent_id'], None)
        self.assertEqual((self.task(other)['parent_id'], self.task(other)['list_id']), (parent, work))
        self.assertEqual(self.task(parent)['subtask_count'], 2)
        self.op('set_parent', id=other, parent_id='')
        self.assertIsNone(self.task(other)['parent_id'])
        # An empty list_id counts as not given: the parent's list is kept.
        inbox_child = self.new('Inbox child', parent_id=parent, list_id='')
        self.assertEqual(self.task(inbox_child)['list_id'], work)

    def test_trash_and_restore_cascade(self):
        parent = self.new('Move house')
        a = self.new('Pack books', parent_id=parent)
        b = self.new('Pack kitchen', parent_id=parent)
        self.op('delete', id=b)  # trashed on its own first
        result = self.op('delete', id=parent)
        self.assertEqual((result['trashed'], result['subtasks']), (2, 1))
        self.assertIsNone(self.task(a))
        trash = {r['id'] for r in self.q('trash')}
        self.assertTrue({parent, a, b} <= trash)
        self.op('delete', id=a)  # already in trash: no-op (bulk selections)
        self.op('restore', id=parent)
        self.assertIsNotNone(self.task(a))
        self.assertIsNone(self.task(b), 'trashed separately, stays in trash')
        self.op('restore', id=a)  # already restored: no-op (bulk undo)
        # Restoring a subtask before its parent keeps it under the parent.
        self.op('delete', id=parent)
        self.op('restore', id=a)
        self.op('restore', id=parent)
        self.assertEqual(self.task(a)['parent_title'], 'Move house')

    def test_dependencies_reject_self_and_cycles(self):
        a, b, c = self.new('Design'), self.new('Build'), self.new('Test')
        with self.assertRaisesRegex(OpError, 'itself'):
            self.op('add_dependency', task_id=a, depends_on=a)
        self.op('add_dependency', task_id=b, depends_on=a)
        self.op('add_dependency', task_id=c, depends_on=b)
        self.assertTrue(self.op('add_dependency', task_id=c, depends_on=b)['existing'])
        with self.assertRaisesRegex(OpError, 'loop'):
            self.op('add_dependency', task_id=a, depends_on=c)
        self.assertEqual(self.task(c)['blocked_by'], 'Build')
        message = self.op('toggle', id=c, done=True)['message']
        self.assertEqual(message, 'Done — was waiting on 1 task')
        self.op('toggle', id=b, done=True)
        self.assertEqual(self.op('toggle', id=a, done=True)['message'], 'Task done')
        self.assertEqual(self.op('remove_dependency', task_id=c, depends_on=b)['removed'], 1)
        self.assertEqual([r['id'] for r in self.q('dependencies', task_id=b)], [a])
        # Trashing a blocker unblocks; restoring it blocks again.
        x, y = self.new('Wait'), self.new('Blocker')
        self.op('add_dependency', task_id=x, depends_on=y)
        self.op('delete', id=y)
        self.assertEqual(self.task(x)['blocked'], 0)
        self.op('restore', id=y)
        self.assertEqual(self.task(x)['blocked'], 1)

    def test_promote_step_and_undo(self):
        parent = self.new('Trip')
        step = self.op('add_step', task_id=parent, title='Book hotel')['id']
        done_step = self.op('add_step', task_id=parent, title='Get visa')['id']
        self.op('toggle_step', id=done_step, done=True)
        made = self.op('promote_step', step_id=step)
        self.assertEqual((made['title'], made['parent_id']), ('Book hotel', parent))
        self.assertEqual([s['id'] for s in self.q('steps', task_id=parent)], [done_step])
        visa = self.op('promote_step', step_id=done_step)['id']
        self.assertEqual(self.task(visa)['done'], 1)
        self.op('unpromote_step', step_id=step, id=made['id'])
        self.assertIsNone(self.task(made['id']))
        self.assertEqual({s['id'] for s in self.q('steps', task_id=parent)}, {step})
        sub_step = self.op('add_step', task_id=visa, title='Photo')['id']
        with self.assertRaisesRegex(OpError, 'one level'):
            self.op('promote_step', step_id=sub_step)

    def test_repeating_parent_does_not_copy_subtasks(self):
        parent = self.new('Weekly review', repeat='weekly')
        self.new('Inbox zero', parent_id=parent)
        nxt = self.op('toggle', id=parent, done=True)['next']['id']
        self.assertEqual(self.q('subtasks', parent_id=nxt), [])
        self.assertEqual(len(self.q('subtasks', parent_id=parent)), 1)
        # A repeating subtask's next occurrence stays under its parent.
        sub = self.new('Water plants', parent_id=parent, repeat='daily')
        again = self.op('toggle', id=sub, done=True)['next']['id']
        self.assertEqual(self.task(again)['parent_id'], parent)


if __name__ == '__main__':
    unittest.main()

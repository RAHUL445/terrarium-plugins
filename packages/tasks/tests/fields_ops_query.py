"""Custom field operations, run for real through `terra preview` (FIX-17).

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


class OpError(Exception):
    pass


@unittest.skipUnless(TERRA and (PACKAGE / 'dist' / 'plugin.wasm').exists(), 'set TERRA_BIN and build the plugin')
class FieldOps(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = tempfile.TemporaryDirectory(prefix='tasks-fields-')
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

    def new(self, title, **args):
        return self.op('create', title=title, **args)['id']

    def field(self, name, type='text', **args):
        return self.op('create_field', name=name, type=type, **args)['id']

    def values(self, task):
        return {r['name']: r for r in self.q('task_field_values', task_id=task)}

    def uniq(self, name):
        self.__class__.n = getattr(self.__class__, 'n', 0) + 1
        return f'{name}-{self.__class__.n}'

    # ---- definitions ------------------------------------------------------

    def test_create_field_validates_and_scopes(self):
        work = self.op('create_list', name=self.uniq('Work'))['id']
        gone = self.op('create_list', name=self.uniq('Gone'))['id']
        self.op('update_list', id=gone, archived=True)
        name = self.uniq('Budget')
        made = self.op('create_field', name=name, type='number', list_id=work, show_on_row=True)
        self.assertEqual((made['type'], made['list_id']), ('number', work))
        with self.assertRaisesRegex(OpError, 'already exists'):
            self.op('create_field', name=name.upper(), type='text', list_id=work)
        # An all-lists field can't reuse the name of a list's field either.
        with self.assertRaisesRegex(OpError, 'already exists'):
            self.op('create_field', name=name, type='text')
        with self.assertRaisesRegex(OpError, 'Type must'):
            self.op('create_field', name=self.uniq('X'), type='money')
        with self.assertRaisesRegex(OpError, 'Name'):
            self.op('create_field', name='   ', type='text')
        with self.assertRaisesRegex(OpError, 'Name'):
            self.op('create_field', name='x' * 101, type='text')
        with self.assertRaisesRegex(OpError, 'active list'):
            self.op('create_field', name=self.uniq('Y'), type='text', list_id=gone)
        with self.assertRaisesRegex(OpError, 'active list'):
            self.op('create_field', name=self.uniq('Y'), type='text', list_id='missing')
        with self.assertRaisesRegex(OpError, 'Only select'):
            self.op('create_field', name=self.uniq('Z'), type='text', options='a\nb')
        shown = {r['id']: r for r in self.q('fields', list_id=work)}
        self.assertEqual(shown[made['id']]['show_on_row'], 1)
        self.assertEqual(shown[made['id']]['scope_label'].startswith('Work'), True)

    def test_select_options_from_lines_commas_and_json(self):
        lines = self.field(self.uniq('Lines'), 'select', options='Design\n  Build \n\nDone')
        commas = self.field(self.uniq('Commas'), 'select', options='Low, High')
        as_json = self.field(self.uniq('Json'), 'select',
                             options=json.dumps(['One', {'id': 'two', 'label': 'Two', 'color': '#ABCDEF'}]))
        self.assertEqual([r['label'] for r in self.q('field_choices', field_id=lines)], ['Design', 'Build', 'Done'])
        self.assertEqual([r['label'] for r in self.q('field_choices', field_id=commas)], ['Low', 'High'])
        rows = self.q('field_choices', field_id=as_json)
        self.assertEqual([(r['value'], r['label'], r['color']) for r in rows[1:]], [('two', 'Two', '#abcdef')])
        with self.assertRaisesRegex(OpError, 'listed twice'):
            self.field(self.uniq('Dup'), 'select', options='A\na')
        with self.assertRaisesRegex(OpError, 'Color'):
            self.field(self.uniq('Bad'), 'select', options=json.dumps([{'label': 'A', 'color': 'red'}]))
        with self.assertRaisesRegex(OpError, 'up to 50'):
            self.field(self.uniq('Many'), 'select', options='\n'.join(f'o{i}' for i in range(51)))

    def test_option_ops_keep_ids_and_clear_removed_values(self):
        stage = self.field(self.uniq('Stage'), 'select', options='Design\nBuild')
        design, build = (r['value'] for r in self.q('field_choices', field_id=stage))
        a, b = self.new('A'), self.new('B')
        self.op('set_field', task_id=a, field_id=stage, value=design)
        self.op('set_field', task_id=b, field_id=stage, value='build')  # a label works too
        self.assertEqual(self.values(b)[self.q('field_detail', id=stage)[0]['name']]['value'], build)
        added = self.op('add_field_option', id=stage, label='Ship', color='#00FF00')['option_id']
        with self.assertRaisesRegex(OpError, 'already an option'):
            self.op('add_field_option', id=stage, label='design')
        self.op('update_field_option', id=stage, option_id=design, label='Draft')
        with self.assertRaisesRegex(OpError, 'already an option'):
            self.op('update_field_option', id=stage, option_id=design, label='Build')
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('update_field_option', id=stage, option_id='nope', label='X')
        name = self.q('field_detail', id=stage)[0]['name']
        self.assertEqual(self.values(a)[name]['display'], 'Draft', 'renaming keeps the task value')
        self.assertEqual([r['label'] for r in self.q('field_choices', field_id=stage)], ['Draft', 'Build', 'Ship'])
        self.assertEqual(self.q('field_choices', field_id=stage)[2]['color'], '#00ff00')
        # Replacing the list keeps ids by label and clears what was dropped.
        result = self.op('update_field', id=stage, options='Draft\nShip\nReview')
        self.assertEqual(result['cleared'], 1)
        ids = {r['label']: r['value'] for r in self.q('field_choices', field_id=stage)}
        self.assertEqual((ids['Draft'], ids['Ship']), (design, added))
        self.assertEqual(self.values(a)[name]['display'], 'Draft')
        self.assertEqual(self.values(b)[name]['value'], '')
        # Removing an option clears its values.
        self.op('set_field', task_id=b, field_id=stage, value=ids['Review'])
        self.assertEqual(self.op('remove_field_option', id=stage, option_id=ids['Review'])['cleared'], 1)
        self.assertEqual(self.values(b)[name]['value'], '')
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('remove_field_option', id=stage, option_id=ids['Review'])
        text = self.field(self.uniq('Plain'))
        with self.assertRaisesRegex(OpError, 'Only select'):
            self.op('add_field_option', id=text, label='x')
        with self.assertRaisesRegex(OpError, 'Only select'):
            self.op('update_field', id=text, options='x')

    def test_update_field_renames_reorders_and_archives(self):
        name = self.uniq('Owner')
        fid = self.field(name, position=1)
        other = self.field(self.uniq('Other'), position=0)
        t = self.new('Task')
        self.op('set_field', task_id=t, field_id=fid, value='me')
        with self.assertRaisesRegex(OpError, 'nothing to update'):
            self.op('update_field', id=fid)
        with self.assertRaisesRegex(OpError, 'already exists'):
            self.op('update_field', id=fid, name=self.q('field_detail', id=other)[0]['name'])
        renamed = self.uniq('Assignee')
        self.op('update_field', id=fid, name=renamed, show_on_row=True)
        self.assertEqual(self.values(t)[renamed]['display'], 'me')
        ordered = [r['id'] for r in self.q('fields', list_id='') if r['id'] in (fid, other)]
        self.assertEqual(ordered, [other, fid])
        self.op('update_field', id=fid, position=0)
        self.op('update_field', id=other, position=9)
        ordered = [r['id'] for r in self.q('fields', list_id='') if r['id'] in (fid, other)]
        self.assertEqual(ordered, [fid, other])
        # Archive: hidden from the panel, row chips and filters; value is kept.
        self.op('update_field', id=fid, archived=True)
        self.assertNotIn(renamed, self.values(t))
        self.assertNotIn(fid, [r['id'] for r in self.q('fields', list_id='')])
        self.assertIsNone(self.q('task', id=t)[0]['fields'] or None)
        with self.assertRaisesRegex(OpError, 'archived'):
            self.op('set_field', task_id=t, field_id=fid, value='you')
        self.op('set_field', task_id=t, field_id=fid, value='')  # clearing still works
        self.op('update_field', id=fid, archived=False)
        self.assertEqual(self.values(t)[renamed]['value'], '')
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('update_field', id='missing', name='x')

    # ---- values -----------------------------------------------------------

    def test_set_field_validates_by_type(self):
        t = self.new('Typed')
        ids = {k: self.field(self.uniq(k), k, **({'options': 'Design\nBuild'} if k == 'select' else {}))
               for k in ('text', 'number', 'date', 'select', 'checkbox', 'url')}

        def put(kind, value):
            return self.op('set_field', task_id=t, field_id=ids[kind], value=value)

        # number
        self.assertEqual(put('number', '12.50')['value'], '12.5')
        self.assertEqual(put('number', 1200)['value'], '1200')
        self.assertEqual(put('number', '1e3')['value'], '1000')
        self.assertEqual(put('number', ' -0.0 ')['value'], '0')
        for bad in ('abc', '12abc', 'inf', 'NaN', '1,5', '1e99'):
            with self.assertRaisesRegex(OpError, 'number|large'):
                put('number', bad)
        # date
        self.assertEqual(put('date', '2026-11-30')['value'], '2026-11-30')
        for bad in ('2026-02-30', '2026-13-01', '30/11/2026', '2026-1-5', 'tomorrow'):
            with self.assertRaisesRegex(OpError, 'date'):
                put('date', bad)
        # url
        self.assertEqual(put('url', 'HTTPS://Example.com/a')['value'], 'HTTPS://Example.com/a')
        for bad in ('example.com', 'ftp://example.com', 'https://', 'https://a b.com', 'javascript:alert(1)'):
            with self.assertRaisesRegex(OpError, 'http'):
                put('url', bad)
        # checkbox
        self.assertEqual(put('checkbox', '1')['value'], '1')
        self.assertEqual(put('checkbox', True)['value'], '1')
        self.assertEqual(put('checkbox', False)['value'], '')
        self.assertEqual(put('checkbox', 'yes')['value'], '1')
        self.assertEqual(put('checkbox', '')['value'], '')
        with self.assertRaisesRegex(OpError, 'checkbox'):
            put('checkbox', 'maybe')
        # select
        design = self.q('field_choices', field_id=ids['select'])[0]['value']
        self.assertEqual(put('select', design)['value'], design)
        with self.assertRaisesRegex(OpError, 'options'):
            put('select', 'Launch')
        # text
        self.assertEqual(put('text', '  hello  ')['value'], 'hello')
        with self.assertRaisesRegex(OpError, '2000'):
            put('text', 'x' * 2001)
        with self.assertRaisesRegex(OpError, 'text, a number'):
            put('text', ['a'])
        # ids
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('set_field', task_id='missing', field_id=ids['text'], value='x')
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('set_field', task_id=t, field_id='missing', value='x')
        self.assertEqual(self.values(t)[self.q('field_detail', id=ids['date'])[0]['name']]['display'][:2], '30')

    def test_set_field_updates_in_place_clears_and_undoes(self):
        fid = self.field(self.uniq('Budget'), 'number')
        name = self.q('field_detail', id=fid)[0]['name']
        t = self.new('Priced')
        first = self.op('set_field', task_id=t, field_id=fid, value='100')
        self.assertEqual((first['previous'], first['value'], first['changed']), ('', '100', True))
        again = self.op('set_field', task_id=t, field_id=fid, value='100')
        self.assertEqual((again['changed'], again['previous']), (False, '100'))
        second = self.op('set_field', task_id=t, field_id=fid, value='250')
        self.assertEqual(second['previous'], '100')
        self.assertEqual([r['id'] for r in self.q('task_field_values', task_id=t)].count(fid), 1)
        # Undo = set the previous value back (what the toast does).
        self.op('set_field', task_id=t, field_id=fid, value=second['previous'])
        self.assertEqual(self.values(t)[name]['value'], '100')
        cleared = self.op('set_field', task_id=t, field_id=fid, value='')
        self.assertEqual((cleared['previous'], cleared['value']), ('100', ''))
        self.assertEqual(self.values(t)[name]['has_value'], 0)
        self.op('set_field', task_id=t, field_id=fid, value=cleared['previous'])
        self.assertEqual(self.values(t)[name]['value'], '100')
        self.assertEqual(self.op('set_field', task_id=t, field_id=fid, value=None)['value'], '')
        self.assertEqual(self.op('set_field', task_id=t, field_id=fid)['changed'], False)

    def test_values_only_for_fields_that_apply(self):
        a = self.op('create_list', name=self.uniq('A'))['id']
        b = self.op('create_list', name=self.uniq('B'))['id']
        fa = self.field(self.uniq('OnlyA'), list_id=a)
        everywhere = self.field(self.uniq('Everywhere'))
        in_a, in_b, in_inbox = self.new('a', list_id=a), self.new('b', list_id=b), self.new('inbox')
        self.op('set_field', task_id=in_a, field_id=fa, value='ok')
        for t in (in_b, in_inbox):
            with self.assertRaisesRegex(OpError, 'another list'):
                self.op('set_field', task_id=t, field_id=fa, value='no')
            self.op('set_field', task_id=t, field_id=everywhere, value='yes')
        self.assertIn(fa, [r['id'] for r in self.q('task_field_values', task_id=in_a)])
        self.assertNotIn(fa, [r['id'] for r in self.q('task_field_values', task_id=in_b)])
        # Moving the task changes which fields it shows; the old value waits.
        self.op('organize', id=in_a, list_id=b)
        self.assertNotIn(fa, [r['id'] for r in self.q('task_field_values', task_id=in_a)])
        self.op('organize', id=in_a, list_id=a)
        self.assertEqual({r['id']: r['value'] for r in self.q('task_field_values', task_id=in_a)}[fa], 'ok')
        # A trashed task can't be edited and keeps its values for restore.
        self.op('delete', id=in_a)
        with self.assertRaisesRegex(OpError, 'no longer exists'):
            self.op('set_field', task_id=in_a, field_id=fa, value='x')
        self.op('restore', id=in_a)
        self.assertEqual({r['id']: r['value'] for r in self.q('task_field_values', task_id=in_a)}[fa], 'ok')

    def test_changing_a_value_reindexes_search_and_rows(self):
        stage = self.field(self.uniq('Phase'), 'select', options='Alpha\nBeta', show_on_row=True)
        tag = self.field(self.uniq('Client'), show_on_row=True)
        t = self.new(self.uniq('Searchable'))
        self.op('set_field', task_id=t, field_id=tag, value='Wonka Industries')
        self.op('set_field', task_id=t, field_id=stage, value='beta')
        hits = [r['id'] for r in self.q('search', q='wonka')]
        self.assertEqual(hits, [t])
        row = next(r for r in self.q('workspace_tasks', today='2026-11-12', filter='everything') if r['id'] == t)
        chips = [row['field_chip_1'], row['field_chip_2'], row['field_chip_3']]
        self.assertEqual(sorted(c for c in chips if c), sorted([f"{self.q('field_detail', id=tag)[0]['name']}: Wonka Industries",
                                                                 f"{self.q('field_detail', id=stage)[0]['name']}: Beta"]))
        summary = self.q('task', id=t)[0]['fields']
        self.assertIn('Wonka Industries', summary)
        self.assertIn('Beta', summary)
        beta = f"{stage}={self.q('field_choices', field_id=stage)[1]['value']}"
        filtered = [r['id'] for r in self.q('workspace_tasks', today='2026-11-12', filter='everything', field_values=beta)]
        self.assertIn(t, filtered)
        alpha = f"{stage}={self.q('field_choices', field_id=stage)[0]['value']}"
        self.assertNotIn(t, [r['id'] for r in self.q('workspace_tasks', today='2026-11-12', filter='everything', field_values=alpha)])

    # ---- export -----------------------------------------------------------

    def test_space_export_carries_both_tables_and_the_csv_column(self):
        fid = self.field(self.uniq('Exported'), 'number')
        name = self.q('field_detail', id=fid)[0]['name']
        t = self.new(self.uniq('Export me'))
        self.op('set_field', task_id=t, field_id=fid, value='42')
        path = Path(self.data.name) / f'export-{time.time_ns()}.zip'
        self.invoke('command', {'kind': 'action', 'id': 'space.export', 'args': {'path': str(path)}})
        with zipfile.ZipFile(path) as z:
            names = z.namelist()
            blob = '\n'.join(z.read(n).decode('utf-8', 'replace') for n in names if n.endswith('.json'))
            self.assertIn('task_fields', blob)
            self.assertIn('task_field_values', blob)
            self.assertIn(fid, blob)
            csv_name = next(n for n in names if n.endswith('tasks.csv'))
            rows = list(csv.DictReader(io.StringIO(z.read(csv_name).decode())))
        mine = next(r for r in rows if r['title'].startswith('Export me'))
        self.assertEqual(mine['custom_fields'], f'{name}: 42')


if __name__ == '__main__':
    unittest.main()

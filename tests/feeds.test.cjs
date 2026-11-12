const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const host = process.env.TERRA_HOST_SOURCE;
if (!host) throw Error('Set TERRA_HOST_SOURCE to a host checkout');
const { rendererHarness } = require(path.join(host, 'ui/tests/helpers/renderer-dom.cjs'));
const manifest = require('../packages/feeds/manifest.json');
const page = require('../packages/feeds/ui/page-main.json');
const widget = require('../packages/feeds/ui/widget-unread.json');

test('feeds declare desktop refresh and explicitly disclose broad network access', () => {
  assert.deepEqual(manifest.hosts, ['*']);
  assert.match(manifest.description, /including sites you have not added/);
  assert.deepEqual(manifest.jobs.refresh, { operation: 'scheduled_refresh', schedule: { every_minutes: 60 }, run_on: 'desktop' });
  assert.equal(manifest.operations.on_fetched.cli, false);
  assert.equal(manifest.operations.scheduled_refresh.cli, false);
  assert.match(manifest.queries.unread.sql, /LIMIT 10$/);
});

for (const [name, template] of [['page', page], ['widget', widget]]) {
  test(`feeds ${name} renders internet content as text, blocks script links and sends explicit read state`, async () => {
    const list = name === 'page' ? page.body.children.find(n => n.query === 'items') : widget.body;
    if (name === 'page') assert.equal(list.group, 'feed_title');
    const ui = rendererHarness();
    const opened = [], actions = [];
    let link = 'javascript:alert(1)';
    const view = ui.renderer.mount(ui.root, { state: {}, body: { ...list, group: undefined } }, {
      today: () => "2026-11-11",
      query: async () => [{ id: 'synthetic', title: '<script>alert(1)</script>', summary: '<img src=x onerror=alert(1)>', link, read: 0 }],
      open: url => opened.push(url), action: async (op, args) => { actions.push({op,args}); return {}; },
      changed() {}, toast() {},
    });
    await view.ready;
    let row = ui.root.children[0].children[0].children[0];
    const main = row.children[1];
    assert.equal(main.children[0].textContent, '<script>alert(1)</script>');
    assert.equal(main.children[1].textContent, '<img src=x onerror=alert(1)>');
    assert.equal(main.children[0].innerHTML, undefined);
    await main.fire('click');
    assert.deepEqual(opened, []);
    row.children[0].checked = true;
    await row.children[0].fire('change');
    assert.equal(actions[0].op, 'mark_read');
    assert.equal(actions[0].args.id, 'synthetic');
    assert.equal(actions[0].args.read, true);
    link = 'https://example.com/story';
    await view.refresh();
    row = ui.root.children[0].children[0].children[0];
    await row.children[1].fire('click');
    assert.deepEqual(opened, ['https://example.com/story']);
    view.dispose();
  });
}

test('feeds page unread filter changes the item query', async () => {
  const ui = rendererHarness();
  const queries = [];
  const view = ui.renderer.mount(ui.root, page, {
    today: () => '2026-11-11', query: async (name, args) => { queries.push({name,args}); return []; },
  });
  await view.ready;
  const checkbox = ui.root.children[0].children.find(n => n.className === 'pp-checkbox').children[0];
  checkbox.checked = true;
  await checkbox.fire('change');
  await view.refresh();
  assert.equal(queries.filter(q => q.name === 'items').at(-1).args.unread, true);
  view.dispose();
});

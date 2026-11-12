const test = require("node:test");
const assert = require("node:assert/strict");

const plugins = [
  { id: "todo", query: "tasks" },
  { id: "notes", query: "list" },
  { id: "bookmarks", query: "list" },
];

function listByQuery(template, query) {
  return template.body.children.filter((node) => node.type === "list" && node.query === query);
}

for (const plugin of plugins) {
  test(`${plugin.id} select actions target bulk-safe operations`, () => {
    const manifest = require(`../packages/${plugin.id}/manifest.json`);
    const template = require(`../packages/${plugin.id}/ui/page-main.json`);
    const lists = listByQuery(template, plugin.query);
    assert.ok(lists.length > 0, `expected at least one ${plugin.query} list`);

    for (const list of lists) {
      assert.ok(list.select, "list should declare select actions");
      assert.ok(Array.isArray(list.select.actions), "select actions should be an array");
      for (const action of list.select.actions) {
        const operation = manifest.operations[action.op];
        assert.ok(operation, `operation ${action.op} must exist`);
        assert.equal(operation.bulk, true, `${action.op} must be marked bulk`);
        if (action.undo) {
          const undoOp = manifest.operations[action.undo.op];
          assert.ok(undoOp, `undo operation ${action.undo.op} must exist`);
          assert.equal(undoOp.bulk, true, `${action.undo.op} must be marked bulk`);
        }
        if (action.confirm) {
          assert.equal(action.variant, "danger", "confirm actions must be danger");
        }
        if (action.ask) {
          const params = operation.params || {};
          assert.ok(
            Object.hasOwn(params, action.ask.name),
            `${action.op} should declare ask param ${action.ask.name}`,
          );
        }
      }
    }
  });
}

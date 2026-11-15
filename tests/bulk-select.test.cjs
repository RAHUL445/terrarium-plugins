const test = require("node:test");
const assert = require("node:assert/strict");

const plugins = [
  { id: "tasks", query: "workspace_tasks", count: 3 },
  { id: "notes", query: "list" },
  { id: "bookmarks", query: "list" },
];

function listByQuery(template, query) {
  const visit = node => {
    if (!node || typeof node !== "object") return [];
    return [node, ...Object.values(node).flatMap(visit)];
  };
  return visit(template.body).filter((node) => node.type === "list" && node.query === query);
}

for (const plugin of plugins) {
  test(`${plugin.id} select actions target bulk-safe operations`, () => {
    const manifest = require(`../packages/${plugin.id}/manifest.json`);
    const template = require(`../packages/${plugin.id}/ui/page-main.json`);
    const lists = listByQuery(template, plugin.query);
    assert.ok(lists.length > 0, `expected at least one ${plugin.query} list`);
    if (plugin.count) assert.equal(lists.length, plugin.count, "list, board and matrix must all retain bulk actions");

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

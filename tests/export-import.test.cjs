const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

for (const entry of fs.readdirSync(path.join(__dirname, "../packages"), {withFileTypes: true})) {
  if (!entry.isDirectory()) continue;
  const id = entry.name;
  const manifest = require(`../packages/${id}/manifest.json`);
  test(`${id} export and import declarations reference existing contracts`, () => {
    for (const declaration of manifest.export || []) {
      assert.ok(manifest.queries[declaration.query]);
      assert.equal(manifest.queries[declaration.query].cli, false);
    }
    for (const declaration of manifest.import || []) {
      assert.ok(manifest.operations[declaration.op]);
    }
  });
}

test("To-do's full CSV importer accepts every exported column", () => {
  const manifest = require("../packages/todo/manifest.json");
  const columns = manifest.export.find(item => item.format === "csv").columns;
  const importer = manifest.import.find(item => item.id === "tasks_csv");
  for (const column of Object.values(importer.fields)) {
    assert.ok(columns.includes(column), column);
  }
  assert.deepEqual(manifest.import.find(item => item.id === "titles_csv").fields, {title: "title"});
  assert.deepEqual(manifest.operations.import.params.repeat.values, manifest.operations.create.params.repeat.values);
});

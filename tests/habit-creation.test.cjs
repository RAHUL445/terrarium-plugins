const test = require("node:test");
const assert = require("node:assert/strict");
const template = require("../packages/habits/ui/page-main.json");
test("habit creation uses a blank record, complete defaults, and a non-saving Cancel", () => {
  const sheet = template.body.children.find(node => node.bind === "state.creating");
  const form = sheet.children.find(node => node.type === "form");
  assert.equal(form.submit.op, "save");
  assert.equal(Object.hasOwn(form.submit.args, "id"), false);
  assert.equal(form.submit.then["state.creating"], "");
  const fields = {};
  function visit(node) { if (node.name) fields[node.name] = node; (node.children || []).forEach(visit); }
  visit(form);
  assert.equal(fields.title.required, true);
  assert.deepEqual(fields.weekdays.value, [0,1,2,3,4,5,6]);
  assert.equal(fields.target.value, 1);
  assert.equal(fields.reminder.value, "");
  assert.deepEqual(sheet.children.find(node => node.label === "Cancel").action, {set:{"state.creating":""}});
});

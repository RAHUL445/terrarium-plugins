const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const host = process.env.TERRA_HOST_SOURCE;
if (!host) throw Error("Set TERRA_HOST_SOURCE to a host checkout for routing integration tests");
const source = fs.readFileSync(require("node:path").join(host, "ui/app.js"), "utf8");
const routing = source.slice(source.indexOf("async function openCreation("), source.indexOf("\nfunction openPalette("));
for (const id of ["habits", "todo", "notes", "bookmarks", "voice"]) {
  const template = require(`../packages/${id}/ui/page-main.json`);
  const manifest = require(`../packages/${id}/manifest.json`);
  test(`${id} declares a creation form that saves a new record and cancels without writes`, () => {
    const creation = template.creation;
    assert.equal(creation.operation, manifest.quick_actions[0].op);
    for (const path of [creation.bind, creation.prefill]) assert.ok(Object.hasOwn(template.state, path.replace(/^state\./, "")));
    const sheet = template.body.children.find(node => node.bind === creation.bind);
    assert.equal(sheet.presentation, "modal");
    assert.equal(sheet.query, undefined);
    const form = sheet.children.find(node => node.type === "form");
    assert.equal(form.submit.op, creation.operation);
    assert.equal(Object.hasOwn(form.submit.args, "id"), false);
    assert.equal(form.submit.then[creation.bind], "");
    assert.deepEqual(sheet.children.find(node => node.label === "Cancel").action, {set:{[creation.bind]:""}});
  });
}
function context(overrides = {}) {
 const changes = [];
 const template = require("../packages/notes/ui/page-main.json");
 const ctx = {currentSpace:"personal", currentPage:{plugin:"notes"}, views:{page:{setState:(...args)=>changes.push(args)}}, $:()=>({hidden:false}), closePalette:()=>{}, openPalette:()=>{}, call:async()=>template, openPage:async()=>true, toast:()=>{}, ...overrides};
 vm.runInNewContext(routing, ctx);
 return {ctx,changes};
}
const plugin = {id:"notes",pages:[{id:"main"}]};
const action = {op:"create",label:"Quick note"};
test("creation route preserves typed text and never invokes a write before submission", async () => {
 const {ctx,changes} = context({call:async (command)=> {assert.equal(command,"view_template"); return require("../packages/notes/ui/page-main.json");}});
 await ctx.openCreation(plugin,action,"Keep this draft");
 assert.deepEqual(changes,[["state.open",""],["state.newText","Keep this draft"],["state.creating","new"]]);
});
test("failed draft flush prevents opening creation", async () => {
 const {ctx,changes} = context({openPage:async()=>false});
 await ctx.openCreation(plugin,action);
 assert.equal(changes.length,0);
});
test("space switch during template loading prevents opening the form in another space", async () => {
 const {ctx,changes} = context();
 ctx.call = async()=>{ctx.currentSpace="work";return require("../packages/notes/ui/page-main.json");};
 await ctx.openCreation(plugin,action);
 assert.equal(changes.length,0);
});

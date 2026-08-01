const fs = require('fs');
let cap = JSON.parse(fs.readFileSync('src-tauri/capabilities/default.json', 'utf-8'));
if (!cap.permissions.includes("dialog:default")) {
  cap.permissions.push("dialog:default");
}
fs.writeFileSync('src-tauri/capabilities/default.json', JSON.stringify(cap, null, 2));

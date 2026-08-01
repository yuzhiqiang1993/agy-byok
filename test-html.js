const fs = require('fs');
let code = fs.readFileSync('src/main.ts', 'utf-8');
const matches = code.match(/closeProviderEditor/g);
console.log("closeProviderEditor calls:", matches ? matches.length : 0);

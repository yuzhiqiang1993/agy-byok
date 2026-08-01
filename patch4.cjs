const fs = require('fs');
let code = fs.readFileSync('src/main.ts', 'utf-8');

// Remove the debug alert we added previously
code = code.replace(
  /console\.log\("!!! CLOSE BUTTON CLICKED"\);\n  alert\("CLOSE BUTTON CLICKED - If you see this, JS is running. The bug is inside closeProviderEditor"\);\n  /g,
  ''
);

fs.writeFileSync('src/main.ts', code);

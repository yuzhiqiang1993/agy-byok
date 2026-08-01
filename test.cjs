const fs = require('fs');
let code = fs.readFileSync('src/main.ts', 'utf-8');

// Replace all event listeners for close to directly log something.
code = code.replace(
  /element<HTMLButtonElement>\(\"#close-provider-modal\"\)\.addEventListener\(\"click\", \(\) => \{\n  void closeProviderEditor\(\);\n\}\);/,
  `element<HTMLButtonElement>("#close-provider-modal").addEventListener("click", () => {
  console.log("!!! CLOSE BUTTON CLICKED");
  alert("CLOSE BUTTON CLICKED - If you see this, JS is running. The bug is inside closeProviderEditor");
  void closeProviderEditor();
});`
);

fs.writeFileSync('src/main.ts', code);

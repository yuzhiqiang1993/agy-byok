const fs = require('fs');
let code = fs.readFileSync('src/main.ts', 'utf-8');

code = code.replace(
  /return !providerEditorDirty \|\| await confirm\("当前有未保存的上游服务修改，确定放弃吗？", \{ kind: 'warning' \}\);/,
  `if (!providerEditorDirty) return true;
  try {
    return await confirm("当前有未保存的上游服务修改，确定放弃吗？", { kind: 'warning' });
  } catch (error) {
    console.error("Native confirm dialog failed:", error);
    return window.confirm("当前有未保存的上游服务修改，确定放弃吗？");
  }`
);

fs.writeFileSync('src/main.ts', code);

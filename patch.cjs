const fs = require('fs');
let code = fs.readFileSync('src/main.ts', 'utf-8');

// Replace synchronous confirmDiscardProviderChanges with async version
code = code.replace(
  /function confirmDiscardProviderChanges\(\): boolean \{[\s\S]*?return !providerEditorDirty \|\| window\.confirm\([^)]+\);\n\}/m,
  `async function confirmDiscardProviderChanges(): Promise<boolean> {
  if (providerEditorBusy) {
    showNotice("上游服务配置正在处理中，请稍候", "error");
    return false;
  }
  return !providerEditorDirty || await confirm("当前有未保存的上游服务修改，确定放弃吗？", { kind: 'warning' });
}`
);

// Fix async calls in closeProviderEditor
code = code.replace(
  /function closeProviderEditor\(force = false\): boolean \{[\s\S]*?if \(!force && !confirmDiscardProviderChanges\(\)\) return false;/,
  `async function closeProviderEditor(force = false): Promise<boolean> {
  if (!force && !(await confirmDiscardProviderChanges())) return false;`
);

// Fix async calls in openProviderEditor
code = code.replace(
  /function openProviderEditor\(providerId: string \| null = null\): void \{/,
  `async function openProviderEditor(providerId: string | null = null): Promise<void> {`
);
code = code.replace(
  /if \(!confirmDiscardProviderChanges\(\)\) return;/,
  `if (!(await confirmDiscardProviderChanges())) return;`
);

// Fix switchTab async calls
code = code.replace(
  /if \(!providerFormPanel\.hidden\) \{\n    if \(!confirmDiscardProviderChanges\(\)\) return;\n    closeProviderEditor\(true\);\n  \}/,
  `if (!providerFormPanel.hidden) {\n    if (!(await confirmDiscardProviderChanges())) return;\n    void closeProviderEditor(true);\n  }`
);
code = code.replace(
  /function switchTab\(targetId: string\): void \{/,
  `async function switchTab(targetId: string): Promise<void> {`
);

// Fix call sites of switchTab
code = code.replace(
  /switchTab\(trigger\.dataset\.target\);/g,
  `void switchTab(trigger.dataset.target);`
);

// Fix call sites of closeProviderEditor
code = code.replace(/closeProviderEditor\(\);/g, `void closeProviderEditor();`);
code = code.replace(/closeProviderEditor\(true\);/g, `void closeProviderEditor(true);`);
code = code.replace(/closeProviderEditor\(\) {/g, `void closeProviderEditor() {`); // skip function definition

fs.writeFileSync('src/main.ts', code);

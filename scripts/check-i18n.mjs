import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import ts from "typescript";

const root = process.cwd();

function readSource(relativePath) {
  return readFileSync(resolve(root, relativePath), "utf8");
}

function propertyName(property) {
  if (!property.name) throw new Error("翻译字典属性缺少名称");
  if (ts.isIdentifier(property.name) || ts.isStringLiteral(property.name)) return property.name.text;
  throw new Error(`不支持的翻译字典属性：${property.name.getText()}`);
}

function findTranslationObject(relativePath, exportName) {
  const source = ts.createSourceFile(relativePath, readSource(relativePath), ts.ScriptTarget.Latest, true);
  let translationObject;

  const visit = (node) => {
    if (
      ts.isVariableDeclaration(node)
      && ts.isIdentifier(node.name)
      && node.name.text === exportName
      && node.initializer
      && ts.isObjectLiteralExpression(node.initializer)
    ) {
      translationObject = node.initializer;
      return;
    }
    ts.forEachChild(node, visit);
  };
  ts.forEachChild(source, visit);

  if (!translationObject) throw new Error(`未找到 ${relativePath} 中的 ${exportName}`);
  return translationObject;
}

function collectLeafValues(object, prefix = "", values = new Map()) {
  for (const property of object.properties) {
    if (!ts.isPropertyAssignment(property)) {
      throw new Error(`翻译字典只允许普通属性：${property.getText()}`);
    }

    const key = prefix ? `${prefix}.${propertyName(property)}` : propertyName(property);
    if (ts.isObjectLiteralExpression(property.initializer)) {
      collectLeafValues(property.initializer, key, values);
      continue;
    }
    if (ts.isStringLiteral(property.initializer) || ts.isNoSubstitutionTemplateLiteral(property.initializer)) {
      if (values.has(key)) throw new Error(`翻译字典包含重复键：${key}`);
      values.set(key, property.initializer.text);
      continue;
    }
    throw new Error(`翻译值必须是字符串：${key}`);
  }
  return values;
}

function assertSameKeys(referenceKeys, localeKeys, localeName) {
  const missing = [...referenceKeys].filter((key) => !localeKeys.has(key));
  const unexpected = [...localeKeys].filter((key) => !referenceKeys.has(key));
  if (missing.length === 0 && unexpected.length === 0) return;

  const details = [
    missing.length > 0 && `缺失：${missing.join(", ")}`,
    unexpected.length > 0 && `多余：${unexpected.join(", ")}`,
  ].filter(Boolean).join("；");
  throw new Error(`${localeName} 翻译键不完整：${details}`);
}

function placeholders(value) {
  return new Set([...value.matchAll(/\{([a-zA-Z][a-zA-Z0-9_]*)\}/g)].map((match) => match[1]));
}

function assertSamePlaceholders(referenceValues, localeValues, localeName) {
  for (const [key, referenceValue] of referenceValues) {
    const referencePlaceholders = placeholders(referenceValue);
    const localePlaceholders = placeholders(localeValues.get(key));
    const missing = [...referencePlaceholders].filter((name) => !localePlaceholders.has(name));
    const unexpected = [...localePlaceholders].filter((name) => !referencePlaceholders.has(name));
    if (missing.length === 0 && unexpected.length === 0) continue;
    throw new Error(
      `${localeName}.${key} 参数不一致：缺失 ${missing.join(", ") || "无"}；多余 ${unexpected.join(", ") || "无"}`,
    );
  }
}

function assertNonEmptyValues(values, localeName) {
  const emptyKeys = [...values]
    .filter(([, value]) => value.trim().length === 0)
    .map(([key]) => key);
  if (emptyKeys.length > 0) {
    throw new Error(`${localeName} 包含空翻译：${emptyKeys.join(", ")}`);
  }
}

function htmlTranslationKeys() {
  const html = readSource("index.html");
  const keys = new Set();
  const pattern = /\bdata-i18n(?:-(?:placeholder|title|aria-label|content))?=(["'])(.*?)\1/g;
  for (const match of html.matchAll(pattern)) keys.add(match[2]);
  return keys;
}

function htmlLineNumber(source, offset) {
  return source.slice(0, offset).split("\n").length;
}

function assertHtmlTranslationCoverage() {
  const html = readSource("index.html");
  const violations = [];
  const attributeMappings = [
    ["placeholder", "data-i18n-placeholder"],
    ["title", "data-i18n-title"],
    ["aria-label", "data-i18n-aria-label"],
  ];

  for (const match of html.matchAll(/<[a-zA-Z][^>]*>/gs)) {
    const tag = match[0];
    for (const [attribute, translationAttribute] of attributeMappings) {
      const valuePattern = new RegExp(`\\b${attribute}=(['\"])(.*?)\\1`, "gs");
      for (const valueMatch of tag.matchAll(valuePattern)) {
        if (valueMatch[2] && !new RegExp(`\\b${translationAttribute}=(['\"])`).test(tag)) {
          violations.push(`${htmlLineNumber(html, match.index)} 行的 ${attribute}`);
        }
      }
    }

    const isDescription = /<meta\b/i.test(tag) && /\bname=(['\"])description\1/i.test(tag);
    if (isDescription && !/\bdata-i18n-content=(['\"])/.test(tag)) {
      violations.push(`${htmlLineNumber(html, match.index)} 行的 description content`);
    }
  }

  const voidElements = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"]);
  const stack = [];
  const tokenPattern = /<!--[\s\S]*?-->|<![^>]*>|<\/?[a-zA-Z][^>]*>|[^<]+/g;
  for (const match of html.matchAll(tokenPattern)) {
    const token = match[0];
    if (token.startsWith("<!--") || token.startsWith("<!")) continue;
    if (token.startsWith("</")) {
      const tagName = token.match(/^<\/([a-zA-Z0-9-]+)/)?.[1]?.toLowerCase();
      while (stack.length > 0) {
        const current = stack.pop();
        if (current.tagName === tagName) break;
      }
      continue;
    }
    if (token.startsWith("<")) {
      const tagName = token.match(/^<([a-zA-Z0-9-]+)/)?.[1]?.toLowerCase();
      if (!tagName || token.endsWith("/>") || voidElements.has(tagName)) continue;
      stack.push({
        tagName,
        declaresText: /\bdata-i18n=(['\"])/.test(token)
          || /\bdata-i18n-static(?:=(['\"])[^'\"]*\1)?/.test(token),
      });
      continue;
    }

    const visibleText = token.replace(/\s+/g, " ").trim();
    if (/\p{L}/u.test(visibleText) && !stack.at(-1)?.declaresText) {
      violations.push(`${htmlLineNumber(html, match.index)} 行的文本“${visibleText}”`);
    }
  }

  if (violations.length > 0) {
    throw new Error(`index.html 包含未声明翻译的用户文案：${violations.join("；")}`);
  }
}

function typescriptFiles(directory) {
  const files = [];
  for (const entry of readdirSync(resolve(root, directory), { withFileTypes: true })) {
    const relativePath = join(directory, entry.name);
    if (entry.isDirectory()) {
      if (relativePath !== join("src", "i18n", "locales")) {
        files.push(...typescriptFiles(relativePath));
      }
    } else if (entry.isFile() && entry.name.endsWith(".ts")) {
      files.push(relativePath);
    }
  }
  return files;
}

function assertTypeScriptTranslationCoverage() {
  const violations = [];
  const userFacingProperties = new Set(["textContent", "title", "placeholder", "ariaLabel"]);

  for (const relativePath of typescriptFiles("src")) {
    const source = ts.createSourceFile(
      relativePath,
      readSource(relativePath),
      ts.ScriptTarget.Latest,
      true,
    );
    const lineNumber = (node) => source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
    const reportLiteral = (node, text, sink) => {
      if (!/\p{L}/u.test(text)) return;
      violations.push(`${relativePath}:${lineNumber(node)} 的 ${sink} 文案“${text}”`);
    };
    const inspectUserFacingExpression = (node, sink) => {
      if (!node) return;
      if (ts.isStringLiteralLike(node)) {
        reportLiteral(node, node.text, sink);
        return;
      }
      if (ts.isTemplateExpression(node)) {
        reportLiteral(node.head, node.head.text, sink);
        for (const span of node.templateSpans) reportLiteral(span.literal, span.literal.text, sink);
        return;
      }
      if (ts.isParenthesizedExpression(node)) {
        inspectUserFacingExpression(node.expression, sink);
        return;
      }
      if (ts.isConditionalExpression(node)) {
        inspectUserFacingExpression(node.whenTrue, sink);
        inspectUserFacingExpression(node.whenFalse, sink);
        return;
      }
      if (ts.isBinaryExpression(node)) {
        inspectUserFacingExpression(node.left, sink);
        inspectUserFacingExpression(node.right, sink);
      }
    };
    const visit = (node) => {
      if (
        ts.isBinaryExpression(node)
        && node.operatorToken.kind === ts.SyntaxKind.EqualsToken
        && ts.isPropertyAccessExpression(node.left)
        && userFacingProperties.has(node.left.name.text)
      ) {
        inspectUserFacingExpression(node.right, node.left.name.text);
      }
      if (
        ts.isCallExpression(node)
        && ts.isIdentifier(node.expression)
        && node.expression.text === "showNotice"
      ) {
        inspectUserFacingExpression(node.arguments[0], "showNotice");
      }
      if (
        ts.isCallExpression(node)
        && ts.isPropertyAccessExpression(node.expression)
        && node.expression.name.text === "setAttribute"
        && ts.isStringLiteralLike(node.arguments[0])
        && ["aria-label", "title", "placeholder"].includes(node.arguments[0].text)
      ) {
        inspectUserFacingExpression(node.arguments[1], `setAttribute(${node.arguments[0].text})`);
      }
      ts.forEachChild(node, visit);
    };
    ts.forEachChild(source, visit);
  }

  if (violations.length > 0) {
    throw new Error(`TypeScript 包含未声明翻译的用户文案：${violations.join("；")}`);
  }
}

function sourceTranslationKeys(referenceKeys) {
  const keys = htmlTranslationKeys();
  for (const relativePath of typescriptFiles("src")) {
    const source = ts.createSourceFile(
      relativePath,
      readSource(relativePath),
      ts.ScriptTarget.Latest,
      true,
    );
    const visit = (node) => {
      if (ts.isStringLiteralLike(node) && referenceKeys.has(node.text)) keys.add(node.text);
      ts.forEachChild(node, visit);
    };
    ts.forEachChild(source, visit);
  }
  return keys;
}

// 构建时同时验证完整 locale 和静态 DOM 使用的翻译键。
const zhCNValues = collectLeafValues(findTranslationObject("src/i18n/locales/zh-CN.ts", "zhCN"));
const enUSValues = collectLeafValues(findTranslationObject("src/i18n/locales/en-US.ts", "enUS"));
const zhCNKeys = new Set(zhCNValues.keys());
const enUSKeys = new Set(enUSValues.keys());
assertNonEmptyValues(zhCNValues, "zh-CN");
assertNonEmptyValues(enUSValues, "en-US");
assertSameKeys(zhCNKeys, enUSKeys, "en-US");
assertSamePlaceholders(zhCNValues, enUSValues, "en-US");

const invalidHtmlKeys = [...htmlTranslationKeys()].filter((key) => !zhCNKeys.has(key));
if (invalidHtmlKeys.length > 0) {
  throw new Error(`index.html 包含未知翻译键：${invalidHtmlKeys.join(", ")}`);
}
assertHtmlTranslationCoverage();
assertTypeScriptTranslationCoverage();

const usedKeys = sourceTranslationKeys(zhCNKeys);
const unusedKeys = [...zhCNKeys].filter((key) => !usedKeys.has(key));
if (unusedKeys.length > 0) {
  throw new Error(`翻译字典包含未使用键：${unusedKeys.join(", ")}`);
}

console.log(`i18n 校验通过：${zhCNKeys.size} 个翻译键，${htmlTranslationKeys().size} 个静态 DOM 引用。`);

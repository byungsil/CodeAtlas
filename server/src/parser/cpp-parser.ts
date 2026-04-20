import Parser from "tree-sitter";
import Cpp from "tree-sitter-cpp";
import { Symbol, SymbolType } from "../models/symbol";

export interface RawCallSite {
  callerId: string;
  calledName: string;
  receiver?: string;
  filePath: string;
  line: number;
}

export interface ParseResult {
  symbols: Symbol[];
  rawCalls: RawCallSite[];
}

const parser = new Parser();
parser.setLanguage(Cpp);

export function parseCppFile(filePath: string, content: string): ParseResult {
  const tree = parser.parse(content);
  const ctx: ParseContext = {
    filePath,
    source: content,
    symbols: [],
    rawCalls: [],
    namespaceStack: [],
  };

  visitNode(tree.rootNode, ctx);
  return { symbols: ctx.symbols, rawCalls: ctx.rawCalls };
}

interface ParseContext {
  filePath: string;
  source: string;
  symbols: Symbol[];
  rawCalls: RawCallSite[];
  namespaceStack: string[];
}

function qualify(ctx: ParseContext, name: string): string {
  const parts = [...ctx.namespaceStack, name].filter(Boolean);
  return parts.join("::");
}

function visitNode(node: Parser.SyntaxNode, ctx: ParseContext): void {
  switch (node.type) {
    case "namespace_definition":
      visitNamespace(node, ctx);
      return;
    case "class_specifier":
      visitClass(node, ctx, "class");
      return;
    case "struct_specifier":
      visitClass(node, ctx, "struct");
      return;
    case "enum_specifier":
      visitEnum(node, ctx);
      return;
    case "function_definition":
      visitFunctionDefinition(node, ctx);
      return;
    case "declaration":
      visitDeclaration(node, ctx);
      return;
  }

  for (let i = 0; i < node.childCount; i++) {
    visitNode(node.child(i)!, ctx);
  }
}

function visitNamespace(node: Parser.SyntaxNode, ctx: ParseContext): void {
  const nameNode = node.childForFieldName("name");
  const bodyNode = node.childForFieldName("body");

  if (nameNode && bodyNode) {
    ctx.namespaceStack.push(nameNode.text);
    for (let i = 0; i < bodyNode.childCount; i++) {
      visitNode(bodyNode.child(i)!, ctx);
    }
    ctx.namespaceStack.pop();
  }
}

function visitClass(node: Parser.SyntaxNode, ctx: ParseContext, kind: "class" | "struct"): void {
  const nameNode = node.childForFieldName("name");
  if (!nameNode) return;

  const className = nameNode.text;
  const classId = qualify(ctx, className);

  ctx.symbols.push({
    id: classId,
    name: className,
    qualifiedName: classId,
    language: "cpp",
    type: kind,
    filePath: ctx.filePath,
    line: node.startPosition.row + 1,
    endLine: node.endPosition.row + 1,
  });

  const body = node.childForFieldName("body");
  if (!body) return;

  ctx.namespaceStack.push(className);

  for (let i = 0; i < body.childCount; i++) {
    const child = body.child(i)!;

    if (child.type === "field_declaration") {
      visitFieldDeclaration(child, ctx, classId);
    } else if (child.type === "function_definition") {
      visitInlineMethodDef(child, ctx, classId);
    } else if (child.type === "declaration") {
      visitMemberDeclaration(child, ctx, classId);
    } else if (child.type === "class_specifier" || child.type === "struct_specifier") {
      visitClass(child, ctx, child.type === "class_specifier" ? "class" : "struct");
    }
  }

  ctx.namespaceStack.pop();
}

function visitFieldDeclaration(node: Parser.SyntaxNode, ctx: ParseContext, parentId: string): void {
  const declarator = findChild(node, "function_declarator");
  if (!declarator) return;

  const nameNode = declarator.childForFieldName("declarator");
  if (!nameNode) return;

  const methodName = nameNode.text;
  const methodId = qualify(ctx, methodName);
  const sig = buildSignature(node);

  ctx.symbols.push({
    id: methodId,
    name: methodName.startsWith("~") ? methodName : methodName,
    qualifiedName: methodId,
    language: "cpp",
    type: "method",
    filePath: ctx.filePath,
    line: node.startPosition.row + 1,
    endLine: node.endPosition.row + 1,
    signature: sig,
    parentId,
  });
}

function visitMemberDeclaration(node: Parser.SyntaxNode, ctx: ParseContext, parentId: string): void {
  const declarator = findDescendant(node, "function_declarator");
  if (!declarator) return;

  const nameNode = declarator.childForFieldName("declarator");
  if (!nameNode) return;

  const methodName = nameNode.text;
  if (methodName.includes("::")) return;

  const methodId = qualify(ctx, methodName);
  const sig = buildSignature(node);

  ctx.symbols.push({
    id: methodId,
    name: methodName,
    qualifiedName: methodId,
    language: "cpp",
    type: "method",
    filePath: ctx.filePath,
    line: node.startPosition.row + 1,
    endLine: node.endPosition.row + 1,
    signature: sig,
    parentId,
  });
}

function visitInlineMethodDef(node: Parser.SyntaxNode, ctx: ParseContext, parentId: string): void {
  const declaratorNode = node.childForFieldName("declarator");
  if (!declaratorNode) return;

  const nameNode = declaratorNode.childForFieldName("declarator");
  if (!nameNode) return;

  const methodName = nameNode.text;
  const methodId = qualify(ctx, methodName);
  const sig = buildFunctionSignature(node);

  ctx.symbols.push({
    id: methodId,
    name: methodName,
    qualifiedName: methodId,
    language: "cpp",
    type: "method",
    filePath: ctx.filePath,
    line: node.startPosition.row + 1,
    endLine: node.endPosition.row + 1,
    signature: sig,
    parentId,
  });

  const body = node.childForFieldName("body");
  if (body) {
    extractCallSites(body, methodId, ctx);
  }
}

function visitEnum(node: Parser.SyntaxNode, ctx: ParseContext): void {
  const nameNode = node.childForFieldName("name");
  if (!nameNode) return;

  const enumName = nameNode.text;
  const enumId = qualify(ctx, enumName);

  ctx.symbols.push({
    id: enumId,
    name: enumName,
    qualifiedName: enumId,
    language: "cpp",
    type: "enum",
    filePath: ctx.filePath,
    line: node.startPosition.row + 1,
    endLine: node.endPosition.row + 1,
  });

  const body = findChild(node, "enumerator_list");
  if (!body) return;

  for (const child of getNamedChildren(body)) {
    if (child.type !== "enumerator") continue;
    const memberNameNode = child.childForFieldName("name");
    if (!memberNameNode) continue;
    const memberName = memberNameNode.text;
    const memberId = `${enumId}::${memberName}`;
    ctx.symbols.push({
      id: memberId,
      name: memberName,
      qualifiedName: memberId,
      language: "cpp",
      type: "enumMember",
      filePath: ctx.filePath,
      line: child.startPosition.row + 1,
      endLine: child.endPosition.row + 1,
      parentId: enumId,
    });
  }
}

function visitFunctionDefinition(node: Parser.SyntaxNode, ctx: ParseContext): void {
  const declaratorNode = node.childForFieldName("declarator");
  if (!declaratorNode) return;

  const funcDeclarator = declaratorNode.type === "function_declarator"
    ? declaratorNode
    : findDescendant(declaratorNode, "function_declarator");
  if (!funcDeclarator) return;

  const nameNode = funcDeclarator.childForFieldName("declarator");
  if (!nameNode) return;

  let funcName: string;
  let funcId: string;
  let parentId: string | undefined;

  if (nameNode.type === "qualified_identifier") {
    const { className, memberName } = parseQualifiedId(nameNode);
    if (className && memberName) {
      funcName = memberName;
      const classId = qualify(ctx, className);
      funcId = `${classId}::${memberName}`;
      parentId = classId;
    } else {
      funcName = nameNode.text;
      funcId = qualify(ctx, funcName);
    }
  } else if (nameNode.type === "destructor_name") {
    funcName = nameNode.text;
    funcId = qualify(ctx, funcName);
  } else {
    funcName = nameNode.text;
    funcId = qualify(ctx, funcName);
  }

  const sig = buildFunctionSignature(node);

  ctx.symbols.push({
    id: funcId,
    name: funcName,
    qualifiedName: funcId,
    language: "cpp",
    type: parentId ? "method" : "function",
    filePath: ctx.filePath,
    line: node.startPosition.row + 1,
    endLine: node.endPosition.row + 1,
    signature: sig,
    parentId,
  });

  const body = node.childForFieldName("body");
  if (body) {
    extractCallSites(body, funcId, ctx);
  }
}

function visitDeclaration(node: Parser.SyntaxNode, ctx: ParseContext): void {
  const declarator = findDescendant(node, "function_declarator");
  if (!declarator) return;

  const nameNode = declarator.childForFieldName("declarator");
  if (!nameNode) return;

  const funcName = nameNode.text;
  if (funcName.includes("::")) return;

  const funcId = qualify(ctx, funcName);
  const sig = buildSignature(node);

  const alreadyExists = ctx.symbols.some((s) => s.id === funcId);
  if (alreadyExists) return;

  ctx.symbols.push({
    id: funcId,
    name: funcName,
    qualifiedName: funcId,
    language: "cpp",
    type: "function",
    filePath: ctx.filePath,
    line: node.startPosition.row + 1,
    endLine: node.endPosition.row + 1,
    signature: sig,
  });
}

function extractCallSites(body: Parser.SyntaxNode, callerId: string, ctx: ParseContext): void {
  visitForCalls(body, callerId, ctx);
}

function visitForCalls(node: Parser.SyntaxNode, callerId: string, ctx: ParseContext): void {
  if (node.type === "call_expression") {
    const funcNode = node.childForFieldName("function");
    if (funcNode) {
      const callInfo = parseCallExpression(funcNode);
      if (callInfo) {
        ctx.rawCalls.push({
          callerId,
          calledName: callInfo.name,
          receiver: callInfo.receiver,
          filePath: ctx.filePath,
          line: node.startPosition.row + 1,
        });
      }
    }
  }

  for (let i = 0; i < node.childCount; i++) {
    visitForCalls(node.child(i)!, callerId, ctx);
  }
}

function parseCallExpression(funcNode: Parser.SyntaxNode): { name: string; receiver?: string } | null {
  if (funcNode.type === "identifier") {
    return { name: funcNode.text };
  }

  if (funcNode.type === "field_expression") {
    const obj = funcNode.childForFieldName("argument");
    const field = funcNode.childForFieldName("field");
    if (field) {
      const receiver = obj ? extractReceiverName(obj) : undefined;
      return { name: field.text, receiver };
    }
  }

  if (funcNode.type === "qualified_identifier") {
    const lastChild = funcNode.namedChild(funcNode.namedChildCount - 1);
    if (lastChild) {
      return { name: lastChild.text };
    }
  }

  return null;
}

function extractReceiverName(node: Parser.SyntaxNode): string | undefined {
  if (node.type === "identifier") return node.text;
  if (node.type === "this") return "this";
  if (node.type === "pointer_expression") {
    const arg = node.childForFieldName("argument");
    if (arg) return extractReceiverName(arg);
  }
  return node.text;
}

function parseQualifiedId(node: Parser.SyntaxNode): { className: string; memberName: string } {
  const parts: string[] = [];
  for (let i = 0; i < node.namedChildCount; i++) {
    parts.push(node.namedChild(i)!.text);
  }
  if (parts.length >= 2) {
    const memberName = parts[parts.length - 1];
    const className = parts.slice(0, -1).join("::");
    return { className, memberName };
  }
  return { className: "", memberName: node.text };
}

function buildFunctionSignature(node: Parser.SyntaxNode): string {
  const declaratorNode = node.childForFieldName("declarator");
  if (!declaratorNode) return "";

  const returnType = buildReturnType(node);
  const funcDecl = declaratorNode.type === "function_declarator"
    ? declaratorNode
    : findDescendant(declaratorNode, "function_declarator");
  if (!funcDecl) return (returnType + " " + declaratorNode.text).trim();

  const nameNode = funcDecl.childForFieldName("declarator");
  const paramsNode = funcDecl.childForFieldName("parameters");
  const qualifiers: string[] = [];
  for (let i = 0; i < funcDecl.childCount; i++) {
    const c = funcDecl.child(i)!;
    if (c.type === "type_qualifier") qualifiers.push(c.text);
  }

  const name = nameNode ? nameNode.text : "";
  const params = paramsNode ? paramsNode.text : "()";
  const qual = qualifiers.length > 0 ? " " + qualifiers.join(" ") : "";
  const prefix = returnType ? returnType + " " : "";

  return `${prefix}${name}${params}${qual}`.trim();
}

function buildReturnType(node: Parser.SyntaxNode): string {
  const parts: string[] = [];
  for (let i = 0; i < node.childCount; i++) {
    const c = node.child(i)!;
    if (c.type === "type_qualifier" || c.type === "primitive_type" ||
        c.type === "type_identifier" || c.type === "qualified_identifier" ||
        c.type === "sized_type_specifier" || c.type === "template_type") {
      parts.push(c.text);
    } else if (c.type === "pointer_declarator" || c.type === "reference_declarator" ||
               c.type === "function_declarator") {
      break;
    }
  }

  const declaratorNode = node.childForFieldName("declarator");
  if (declaratorNode) {
    if (declaratorNode.type === "reference_declarator") {
      const inner = findDescendant(declaratorNode, "function_declarator");
      if (inner) {
        parts.push("&");
      }
    } else if (declaratorNode.type === "pointer_declarator") {
      parts.push("*");
    }
  }

  return parts.join(" ");
}

function buildSignature(node: Parser.SyntaxNode): string {
  let text = node.text;
  text = text.replace(/\s*[{;]\s*$/, "").trim();
  const braceIdx = text.indexOf("{");
  if (braceIdx !== -1) text = text.substring(0, braceIdx).trim();
  return text;
}

function findChild(node: Parser.SyntaxNode, type: string): Parser.SyntaxNode | null {
  for (let i = 0; i < node.childCount; i++) {
    if (node.child(i)!.type === type) return node.child(i);
  }
  return null;
}

function findDescendant(node: Parser.SyntaxNode, type: string): Parser.SyntaxNode | null {
  if (node.type === type) return node;
  for (let i = 0; i < node.childCount; i++) {
    const found = findDescendant(node.child(i)!, type);
    if (found) return found;
  }
  return null;
}

function getNamedChildren(node: Parser.SyntaxNode): Parser.SyntaxNode[] {
  const children: Parser.SyntaxNode[] = [];
  for (let index = 0; index < node.namedChildCount; index += 1) {
    const child = node.namedChild(index);
    if (child) {
      children.push(child);
    }
  }
  return children;
}

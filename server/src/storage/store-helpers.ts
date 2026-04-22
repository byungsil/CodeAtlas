import { Symbol } from "../models/symbol";
import {
  BaseMethodRecord,
  MatchReason,
  OverrideRecord,
  TypeHierarchyNode,
} from "../models/responses";

export function toHierarchyNode(symbol: Symbol): TypeHierarchyNode {
  return {
    symbolId: symbol.id,
    qualifiedName: symbol.qualifiedName,
    type: symbol.type,
    filePath: symbol.filePath,
    line: symbol.line,
  };
}

export function compareOverrideRecords(left: BaseMethodRecord | OverrideRecord, right: BaseMethodRecord | OverrideRecord): number {
  return left.owner.qualifiedName.localeCompare(right.owner.qualifiedName)
    || left.method.qualifiedName.localeCompare(right.method.qualifiedName);
}

export function inferOverrideConfidence(
  derivedMethod: Symbol,
  baseMethod: Symbol,
): { confidence: "high" | "partial"; matchReasons: MatchReason[] } {
  const matchReasons: MatchReason[] = [
    "override_inheritance_match",
    "override_name_match",
  ];

  if (
    derivedMethod.parameterCount !== undefined
    && baseMethod.parameterCount !== undefined
    && derivedMethod.parameterCount === baseMethod.parameterCount
  ) {
    matchReasons.push("override_parameter_count_match");
    return { confidence: "high", matchReasons };
  }

  const derivedArity = inferSignatureArity(derivedMethod.signature);
  const baseArity = inferSignatureArity(baseMethod.signature);
  if (derivedArity !== undefined && derivedArity === baseArity) {
    matchReasons.push("override_signature_arity_match");
    return { confidence: "high", matchReasons };
  }

  return { confidence: "partial", matchReasons };
}

export function inferSignatureArity(signature?: string): number | undefined {
  if (!signature) return undefined;
  const start = signature.indexOf("(");
  const end = signature.lastIndexOf(")");
  if (start < 0 || end <= start) return undefined;
  const params = signature.slice(start + 1, end).trim();
  if (!params || params === "void") return 0;
  return params.split(",").length;
}

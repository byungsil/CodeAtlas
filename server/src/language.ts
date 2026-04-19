import { SourceLanguage } from "./models/symbol";

const CPP_EXTENSIONS = new Set(["c", "cpp", "h", "hpp", "cc", "cxx", "inl", "inc"]);

export function deriveLanguageFromPath(filePath: string): SourceLanguage {
  const normalized = filePath.toLowerCase();
  const extension = normalized.includes(".")
    ? normalized.slice(normalized.lastIndexOf(".") + 1)
    : "";

  if (CPP_EXTENSIONS.has(extension)) return "cpp";
  if (extension === "lua") return "lua";
  if (extension === "py") return "python";
  if (extension === "ts" || extension === "tsx") return "typescript";
  if (extension === "rs") return "rust";
  return "cpp";
}

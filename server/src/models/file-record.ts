export interface FileRecord {
  path: string;
  contentHash: string;
  lastIndexed: string;
  symbolCount: number;
  module?: string;
  subsystem?: string;
  projectArea?: string;
  artifactKind?: "runtime" | "editor" | "tool" | "test" | "generated";
  headerRole?: "public" | "private" | "internal";
}

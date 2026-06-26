export type ResolutionTier = "compilerConfirmed" | "heuristic" | "chaVirtual";

export interface Call {
  callerId: string;
  calleeId: string;
  filePath: string;
  line: number;
  /**
   * How confidently the callee was resolved.
   * `compilerConfirmed` = libclang USR fast-path; `heuristic` = name-based scoring;
   * `chaVirtual` = MS27 Class Hierarchy Analysis edge (a virtual call expanded to
   * a concrete override in the static type's subtree — a sound over-approximation).
   * Stored in SQLite as `compiler_confirmed` / `heuristic` / `cha_virtual`.
   */
  resolutionTier: ResolutionTier;
}

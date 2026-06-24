export type ResolutionTier = "compilerConfirmed" | "heuristic";

export interface Call {
  callerId: string;
  calleeId: string;
  filePath: string;
  line: number;
  /**
   * How confidently the callee was resolved.
   * `compilerConfirmed` = libclang USR fast-path; `heuristic` = name-based scoring.
   * Stored in SQLite as `compiler_confirmed` / `heuristic`.
   */
  resolutionTier: ResolutionTier;
}

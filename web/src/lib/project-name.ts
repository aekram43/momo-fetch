/**
 * The folder a project is in, as a person would name it.
 *
 * The file tree addresses the project root as `"."`, which is correct for the
 * API and useless as a label — it renders as a lone dot above the listing. The
 * settings endpoint knows the absolute path, so the panel can say
 * `momo-assistant` instead.
 */
export function projectName(projectPath: string | undefined): string | null {
  if (!projectPath) return null;

  // Both separators: the desktop shell runs on Windows too, and a path from
  // there arrives with backslashes.
  // Keep the original when trimming would empty it — a project at "/" still
  // needs something to show.
  const trimmed = projectPath.replace(/[\\/]+$/, "") || projectPath;
  const name = trimmed.split(/[\\/]/).pop();

  // A project rooted at `/` has no folder name to show; the path itself is the
  // most honest label left.
  return name || trimmed || null;
}

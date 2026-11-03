// The tokens an upload link's subfolder template can use; the API renders them when a client's
// session starts (src/services/upload_paths.rs).

// `asks`: the upload-page field the token renders, which a template using it always asks for.
export type Token = { name: string; desc: string; asks?: "name" | "project" };

export const UPLOAD_TOKENS: Token[] = [
  { name: "date", desc: "YYYY-MM-DD" },
  { name: "year", desc: "YYYY" },
  { name: "month", desc: "MM" },
  { name: "day", desc: "DD" },
  { name: "time", desc: "24-hour time, HHhMM (09h30)" },
  { name: "uploader_name", desc: "Name entered by the uploader", asks: "name" },
  { name: "project", desc: "Project entered by the uploader", asks: "project" },
];

/** The upload-page fields a template's tokens need (the API asks for them the same way). */
export function templateAsks(template: string | null | undefined): { name: boolean; project: boolean } {
  const uses = (field: Token["asks"]) => UPLOAD_TOKENS.some((t) => t.asks === field && !!template?.includes(`{${t.name}}`));
  return { name: uses("name"), project: uses("project") };
}

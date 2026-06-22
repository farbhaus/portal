// Path-template tokens, mirroring the backend's _tokens() in apps/api/src/portal/sync/paths.py.
// Sync rules can use every token; upload links only render the date parts and {uploader_name}.

export type Token = { name: string; desc: string };

export const SYNC_TOKENS: Token[] = [
  { name: "filename", desc: "Full name with extension (clip.mov)" },
  { name: "stem", desc: "Name without extension (clip)" },
  { name: "ext", desc: "Extension without the dot (mov)" },
  { name: "project", desc: "Frame.io project name" },
  { name: "folder", desc: "Source folder name" },
  { name: "subfolder", desc: "Source path below the root folder (Dailies/Monday)" },
  { name: "date", desc: "YYYY-MM-DD" },
  { name: "year", desc: "YYYY" },
  { name: "month", desc: "MM" },
  { name: "day", desc: "DD" },
  { name: "time", desc: "24-hour time, HHhMM (09h30)" },
];

export const UPLOAD_TOKENS: Token[] = [
  { name: "date", desc: "YYYY-MM-DD" },
  { name: "year", desc: "YYYY" },
  { name: "month", desc: "MM" },
  { name: "day", desc: "DD" },
  { name: "time", desc: "24-hour time, HHhMM (09h30)" },
  { name: "uploader_name", desc: "Name entered by the uploader" },
];
